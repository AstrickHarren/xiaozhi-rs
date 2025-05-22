#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::ops::Deref;

use bytes::Buf;
use bytes::BytesMut;
use embassy_executor::Spawner;
use embassy_net::udp::PacketMetadata;
use embassy_net::udp::UdpSocket;
use embassy_net::{IpEndpoint, Stack};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::channel::Receiver;
use embassy_sync::channel::Sender;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::dma::DmaPriority;
use esp_hal::i2s::master::{I2s, I2sTx};
use esp_hal::peripheral::PeripheralRef;
use esp_hal::peripherals::I2S1;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_circular_buffers_chunk_size, Async};
use esp_println::print;
use esp_println::println;
use firmware::p3::P3Reader;
use firmware::wifi::{WifiConfig, WifiConnection};
use log::{debug, error, info, warn, LevelFilter};
use opus::Decoder;

// When you are okay with using a nightly compiler, it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

static wifi_config_p3: &[u8] = include_bytes!("../../assets/wificonfig.p3");

#[esp_hal_embassy::main]
async fn main(s: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);
    esp_alloc::heap_allocator!(size: 100 * 1024);
    esp_println::logger::init_logger(LevelFilter::Info);

    // Start the WI-FI
    // info!("Connecting to Wifi");
    // let stack = {
    //     let cfg = WifiConfig {
    //         ssid: "Thunderstorm",
    //         password: "12345678".into(),
    //         wifi: peripherals.WIFI,
    //         timg: peripherals.TIMG0,
    //         rng: peripherals.RNG,
    //         radio_clk: peripherals.RADIO_CLK,
    //     };
    //     let stack = WifiConnection::connect(s, cfg).await;
    //     info!("Waiting for IP");
    //     stack.wait_config_up().await;
    //     let ip = stack.config_v4().unwrap().address;
    //     info!("Got IP: {}", ip);
    //     stack
    // };

    // Set up I2S
    let (rx_buf, i2s_tx) = {
        let (rx_buf, rx_d, _, tx_d) = dma_circular_buffers_chunk_size!(1024 * 64, 1024 * 32, 1024);
        // peripherals.I2S1.register_block().tx_conf();
        let i2s = I2s::new(
            peripherals.I2S1,
            esp_hal::i2s::master::Standard::Philips,
            esp_hal::i2s::master::DataFormat::Data16Channel16,
            Rate::from_khz(16),
            peripherals.DMA_CH0,
            rx_d,
            tx_d,
        )
        .into_async();
        let i2s_tx = {
            let mut i2s = i2s
                .i2s_tx
                .with_ws(peripherals.GPIO16)
                .with_bclk(peripherals.GPIO15)
                .with_dout(peripherals.GPIO7);
            i2s.tx_channel.set_priority(DmaPriority::Priority0);
            i2s.build()
        };
        (rx_buf, i2s_tx)
    };

    // Set up channel
    let ch = &*mk_static! {
        Channel::<NoopRawMutex, BytesMut, 10>,
        Channel::new()
    };
    let sender = ch.sender();
    let receiver = ch.receiver();
    s.spawn(audio_task(receiver, i2s_tx, rx_buf)).unwrap();

    // udp_play(stack, sender).await;
    local_play(sender).await;

    Timer::after(Duration::from_millis(1000)).await;
    warn!("Sleeping!");
    loop {}
}

pub struct DummyTimeSource;

impl embedded_sdmmc::TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp::from_fat(0, 0)
    }
}

#[embassy_executor::task]
async fn audio_task(
    receiver: Receiver<'static, NoopRawMutex, BytesMut, 10>,
    i2s_tx: I2sTx<'static, Async>,
    rx_buf: &'static mut [u8],
) {
    const SILENSE_SIZE: usize = 100;
    let max_silense_size = rx_buf.len();
    let mut transfer = i2s_tx.write_dma_circular_async(rx_buf).unwrap();

    let mut i2s_zero_bytes = 0;
    loop {
        debug!("queued {} audio samples", receiver.len());

        let mut data = match receiver.try_receive() {
            Ok(p) => p,
            Err(_) if i2s_zero_bytes >= max_silense_size => {
                i2s_zero_bytes = 0;
                receiver.receive().await
            }
            Err(_) => {
                i2s_zero_bytes += SILENSE_SIZE;
                let mut silent = BytesMut::with_capacity(SILENSE_SIZE);
                silent.resize(SILENSE_SIZE, 0);
                silent
            }
        };

        // Push all bytes (audio or silence) into I²S
        while !data.is_empty() {
            let n = transfer.push(&data).await.unwrap();
            data.advance(n);
        }
    }
}

async fn local_play(sender: Sender<'static, NoopRawMutex, BytesMut, 10>) {
    let reader = P3Reader::new(wifi_config_p3);
    let pcm = &mut [0; 960];
    let mut dec = Decoder::new(16000, opus::Channels::Mono).unwrap();
    for packet in reader {
        let buf = packet.unwrap();
        let n = dec.decode(&buf, pcm, false).unwrap();

        let pcm: BytesMut = pcm
            .into_iter()
            .take(n)
            .flat_map(|x| [*x, *x])
            .flat_map(|x| x.to_le_bytes())
            .collect();
        sender.try_send(pcm).unwrap();

        Timer::after_millis(55).await;
    }
}

async fn udp_play(
    stack: Stack<'_>,
    sender: Sender<'static, NoopRawMutex, BytesMut, 10>,
    // i2s_tx: I2sTx<'_, Async>,
    // rx_buf: &mut [u8],
) {
    // UDP get wav file
    const UDP_BUF_SIZE: usize = 4096;
    let (rx, tx, rx_meta, tx_meta) = (
        &mut [0; UDP_BUF_SIZE],
        &mut [0; UDP_BUF_SIZE],
        &mut [PacketMetadata::EMPTY; 2],
        &mut [PacketMetadata::EMPTY; 2],
    );
    let mut udp = UdpSocket::new(stack, rx_meta, rx, tx_meta, tx);
    let addr = stack.config_v4().unwrap().address.address();
    udp.bind(IpEndpoint::new(addr.into(), 8080))
        .inspect_err(|e| error!("Failed to bind UDP socket: {e:?}"))
        .ok();

    info!("Waiting for udp packets");
    let buf = &mut [0; 1024];
    let pcm = &mut [0; 960];
    let mut dec = Decoder::new(16000, opus::Channels::Mono).unwrap();
    // let mut transfer = i2s_tx.write_dma_circular_async(rx_buf).unwrap();

    loop {
        // debug!("waiting for next udp packet");
        match udp
            .recv_from(buf)
            .await
            .inspect_err(|e| error!("Failed to receive UDP packet: {e:?}"))
        {
            Ok((n, _)) if n > 0 => {
                // debug!("Udp recved {n} bytes");
                let buf = &mut buf[..n];
                let n = dec.decode(buf, pcm, false).unwrap();
                let pcm: BytesMut = pcm
                    .into_iter()
                    .take(n)
                    .flat_map(|x| [*x, *x])
                    .flat_map(|x| x.to_le_bytes())
                    .collect();
                sender.try_send(pcm).unwrap();
                // let pcm =
                //     unsafe { core::slice::from_raw_parts_mut(pcm.as_mut_ptr() as *mut u8, n * 2) };

                // while transfer.available().await.unwrap() < pcm.len() {}
                // let n = transfer
                //     .push(&pcm)
                //     .await
                //     .inspect_err(|e| error!("Failed to push buffer: {e:?}"))
                //     .unwrap();
                // assert!(n == pcm.len())
            }
            _ => break,
        }
    }
}
