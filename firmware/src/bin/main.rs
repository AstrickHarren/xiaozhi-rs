#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

use core::str::{self, FromStr};

use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::IpEndpoint;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_println::println;
use firmware::wifi::{WifiConfig, WifiConnection};
use log::{error, info, LevelFilter};

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

    // Make a TCP connection
    let (rx, tx) = (&mut [0; 4096], &mut [0; 4096]);
    let mut tcp = TcpSocket::new(stack, rx, tx);
    tcp.set_timeout(Some(Duration::from_secs(10)));
    info!("making tcp connection");
    tcp.connect(IpEndpoint::from_str("142.250.185.115:80").unwrap())
        .await
        .inspect_err(|e| error!("{e:?}"))
        .ok();
    info!("tcp connected");
    tcp.write(b"GET / HTTP/1.1\r\nHost: www.mobile-j.de\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let msg = tcp.read(&mut buf).await.unwrap();
    info!("got response: {:?}", str::from_utf8(&buf[..msg]).unwrap());
    esp_hal::i2s::master::DataFormat::Data16Channel16;

    Timer::after(Duration::from_millis(1000)).await;
    println!("Bing!");

    loop {}
}

pub struct DummyTimeSource;

impl embedded_sdmmc::TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> embedded_sdmmc::Timestamp {
        embedded_sdmmc::Timestamp::from_fat(0, 0)
    }
}
