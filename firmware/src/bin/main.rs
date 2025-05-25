#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]

use core::ops::Deref;
use core::ptr::slice_from_raw_parts;

use bytes::Buf;
use bytes::BufMut;
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
use esp_hal::dma::DmaError;
use esp_hal::dma::DmaPriority;
use esp_hal::dma_buffers;
use esp_hal::dma_circular_buffers;
use esp_hal::i2s;
use esp_hal::i2s::master::I2sRx;
use esp_hal::i2s::master::{I2s, I2sTx};
use esp_hal::peripheral::PeripheralRef;
use esp_hal::peripherals::I2S1;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_circular_buffers_chunk_size, Async};
use esp_println::dbg;
use esp_println::print;
use esp_println::println;
use firmware::p3::P3Reader;
use firmware::wifi::{WifiConfig, WifiConnection};
use log::{debug, error, info, warn, LevelFilter};
use opus::Decoder;
use opus::Encoder;

// When you are okay with using a nightly compiler, it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

static WIFI_CONFIG_P3: &[u8] = include_bytes!("../../assets/wificonfig.p3");

#[esp_hal_embassy::main]
async fn main(s: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);
    esp_alloc::heap_allocator!(size: 100 * 1024);
    esp_println::logger::init_logger(LevelFilter::Debug);

    // Start the WI-FI
    info!("Connecting to Wifi");
    let stack = {
        let cfg = WifiConfig {
            ssid: "Thunderstorm",
            password: "12345678".into(),
            wifi: peripherals.WIFI,
            timg: peripherals.TIMG0,
            rng: peripherals.RNG,
            radio_clk: peripherals.RADIO_CLK,
        };
        let stack = WifiConnection::connect(s, cfg).await;
        info!("Waiting for IP");
        stack.wait_config_up().await;
        let ip = stack.config_v4().unwrap().address;
        info!("Got IP: {}", ip);
        stack
    };
    const UDP_BUF_SIZE: usize = 4096;
    let (rx, tx, rx_meta, tx_meta) = (
        &mut [0; UDP_BUF_SIZE],
        &mut [0; UDP_BUF_SIZE],
        &mut [PacketMetadata::EMPTY; 10],
        &mut [PacketMetadata::EMPTY; 10],
    );
    let mut udp = UdpSocket::new(stack, rx_meta, rx, tx_meta, tx);
    let addr = stack.config_v4().unwrap().address.address();
    udp.bind(IpEndpoint::new(addr.into(), 8080))
        .inspect_err(|e| error!("Failed to bind UDP socket: {e:?}"))
        .ok();

    // Set up I2S for speaker
    let (tx_buf, i2s_tx) = {
        let (_, rx_d, tx_buf, tx_d) = dma_buffers!(0, 4092 * 4);
        let i2s = I2s::new(
            peripherals.I2S1,
            esp_hal::i2s::master::Standard::Philips,
            esp_hal::i2s::master::DataFormat::Data32Channel32,
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
        (tx_buf, i2s_tx)
    };

    // Set up I2S for mic
    let (rx_buf, i2s_rx) = {
        let (rx_buf, rx_d, _, tx_d) = dma_circular_buffers!(4092 * 10, 0);
        let i2s = I2s::new(
            peripherals.I2S0,
            esp_hal::i2s::master::Standard::Philips,
            esp_hal::i2s::master::DataFormat::Data32Channel32,
            Rate::from_khz(16),
            peripherals.DMA_CH1,
            rx_d,
            tx_d,
        )
        .into_async();
        let i2s_rx = {
            let mut i2s = i2s
                .i2s_rx
                .with_ws(peripherals.GPIO4)
                .with_bclk(peripherals.GPIO5)
                .with_din(peripherals.GPIO6);
            i2s.rx_channel.set_priority(DmaPriority::Priority0);
            i2s.build()
        };
        (rx_buf, i2s_rx)
    };

    // Set up channel
    let ch = &*mk_static! {
        Channel::<NoopRawMutex, BytesMut, 10>,
        Channel::new()
    };
    let sender = ch.sender();
    let receiver = ch.receiver();
    // s.spawn(speak_task(receiver, i2s_tx, tx_buf)).unwrap();

    // udp_play(&udp, sender).await;
    // local_play(sender).await;
    listen(&udp, "172.20.10.8:8080".parse().unwrap(), i2s_rx, rx_buf).await;

    Timer::after(Duration::from_millis(1000)).await;
    warn!("Sleeping!");
    loop {}
}

async fn listen(
    udp: &UdpSocket<'_>,
    remote: IpEndpoint,
    i2s_rx: I2sRx<'static, Async>,
    buf: &mut [u8],
) {
    let enc = Encoder::new(16000, opus::Channels::Mono, opus::Application::Audio).unwrap();
    let mut data = [0; 4096];
    let mut transfer = i2s_rx.read_dma_circular_async(buf).unwrap();
    loop {
        use esp_hal::i2s::master::Error;
        match transfer.pop(&mut data).await {
            Ok(n) => {
                // INMP441 puts data into 24bits
                let data = data[..n]
                    .array_chunks::<4>()
                    .step_by(2) // skip right channel
                    .map(|c| i32::from_le_bytes(*c) >> 12) // shift right by 8 bits (Big Endian)
                    .map(|c| c.clamp(i16::MIN as _, i16::MAX as _) as i16);

                let data: BytesMut = data.flat_map(|b| b.to_le_bytes()).collect();
                udp.send_to(&data, remote).await.unwrap();
                debug!("sent udp to {:?}: {} bytes", remote, data.len());
            }
            Err(Error::DmaError(DmaError::Late)) => warn!("Dma late for mic"),
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }
}

#[embassy_executor::task]
async fn speak_task(
    receiver: Receiver<'static, NoopRawMutex, BytesMut, 10>,
    i2s_tx: I2sTx<'static, Async>,
    tx_buf: &'static mut [u8],
) {
    const SILENSE_SIZE: usize = 100;
    let max_silense_size = 300;
    let mut transfer = i2s_tx.write_dma_circular_async(tx_buf).unwrap();

    let mut i2s_zero_bytes = 0;
    let pcm = &mut [0; 960];
    // FIXME: need to reset decoder every time a new udp stream is received
    let mut dec = Decoder::new(16000, opus::Channels::Mono).unwrap();
    loop {
        debug!("queued {} audio samples", receiver.len());

        match receiver.try_receive() {
            Ok(data) => dec.decode(&data, pcm, false).unwrap(),
            Err(_) => dec.decode(&[], pcm, false).unwrap(),
        };

        let volume_factor: i32 = 32112; // WARNING: 70% volume
        let mut data: BytesMut = pcm
            .into_iter()
            .map(|p| {
                let temp = *p as i64 * volume_factor as i64;
                // clamp to i32 range
                let clamped = temp.max(i32::MIN as i64).min(i32::MAX as i64) as i32;
                clamped as i32
            })
            .flat_map(|x| [0, x])
            .flat_map(|x| x.to_le_bytes())
            .collect();

        // Push all bytes (audio or silence) into I²S
        while !data.is_empty() {
            let n = transfer.push(&data).await.unwrap();
            data.advance(n);
        }
    }
}

async fn local_play(sender: Sender<'static, NoopRawMutex, BytesMut, 10>) {
    let reader = P3Reader::new(WIFI_CONFIG_P3);
    for packet in reader {
        sender.try_send(packet.unwrap()).unwrap();
        Timer::after_millis(55).await;
    }
}

async fn udp_play(
    udp: &UdpSocket<'_>,
    sender: Sender<'static, NoopRawMutex, BytesMut, 10>,
    // i2s_tx: I2sTx<'_, Async>,
    // rx_buf: &mut [u8],
) {
    let buf = &mut [0; 1024];

    info!("Waiting for udp packets");
    loop {
        // debug!("waiting for next udp packet");
        match udp
            .recv_from(buf)
            .await
            .inspect_err(|e| error!("Failed to receive UDP packet: {e:?}"))
        {
            Ok((n, _)) if n > 0 => {
                // debug!("Udp recved {n} bytes");
                let src = &mut buf[..n];
                let mut buf: BytesMut = BytesMut::with_capacity(n);
                buf.put_slice(src);
                sender.send(buf).await
            }
            _ => break,
        }
    }
}
