#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::str::FromStr;

use embassy_executor::Spawner;
use embassy_net::udp::PacketMetadata;
use embassy_net::udp::UdpSocket;
use embassy_net::{IpEndpoint, Stack};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::i2s::master::{I2s, I2sTx};
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use esp_hal::{dma_circular_buffers_chunk_size, Async};
use firmware::wifi::{WifiConfig, WifiConnection};
use log::{debug, error, info, warn, LevelFilter};

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
            ssid: "Wokwi-GUEST",
            password: None,
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

    // Set up I2S
    let (rx_buf, i2s_tx) = {
        let (rx_buf, rx_d, _, tx_d) = dma_circular_buffers_chunk_size!(512 * 6, 512 * 6, 512);
        let i2s = I2s::new(
            peripherals.I2S1,
            esp_hal::i2s::master::Standard::Philips,
            esp_hal::i2s::master::DataFormat::Data16Channel16,
            Rate::from_khz(16),
            peripherals.DMA_I2S0,
            rx_d,
            tx_d,
        )
        .into_async();
        let i2s_tx = i2s
            .i2s_tx
            .with_ws(peripherals.GPIO16)
            .with_bclk(peripherals.GPIO15)
            .with_dout(peripherals.GPIO7)
            .build();
        (rx_buf, i2s_tx)
    };

    udp_play(stack, i2s_tx, rx_buf).await;

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

async fn udp_play(stack: Stack<'_>, i2s_tx: I2sTx<'_, Async>, rx_buf: &mut [u8]) {
    // UDP get wav file
    const UDP_BUF_SIZE: usize = 4096;
    let (rx, tx, rx_meta, tx_meta) = (
        &mut [0; UDP_BUF_SIZE],
        &mut [0; UDP_BUF_SIZE],
        &mut [PacketMetadata::EMPTY; 2],
        &mut [PacketMetadata::EMPTY; 2],
    );
    let mut udp = UdpSocket::new(stack, rx_meta, rx, tx_meta, tx);
    udp.bind(IpEndpoint::from_str("192.168.31.83:8080").unwrap())
        .inspect_err(|e| error!("Failed to bind UDP socket: {e:?}"))
        .ok();

    info!("Waiting for udp packets");
    let buf = &mut [0; 1024];
    let _pcm = &mut [0; 2048];
    // let mut dec = Decoder::new(16000, opus::Channels::Mono).unwrap();
    let mut transfer = i2s_tx.write_dma_circular_async(rx_buf).unwrap();
    loop {
        match udp
            .recv_from(buf)
            .await
            .inspect_err(|e| error!("Failed to receive UDP packet: {e:?}"))
        {
            Ok((n, _)) if n > 0 => {
                debug!("Udp recved {n} bytes");
                let buf = &mut buf[..n];
                //let n = dec.decode(buf, pcm, false).unwrap();
                //let mut pcm: BytesMut = pcm
                //    .into_iter()
                //    .take(n * 2)
                //    .flat_map(|x| x.to_le_bytes())
                //    .collect();

                transfer
                    .push(buf)
                    .await
                    .inspect_err(|e| error!("Failed to push buffer: {e:?}"))
                    .ok();
                // i2s_tx.write_dma_async(buf).await.ignore_or_debug();
            }
            _ => break,
        }
    }
}
