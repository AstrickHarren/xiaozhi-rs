#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]
#![feature(inherent_str_constructors)]
#![feature(concat_bytes)]

use embassy_executor::Spawner;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::tcp::client::TcpClientState;
use embedded_websocket::EmptyRng;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::peripheral::Peripheral;
use esp_hal::rng::Trng;
use esp_hal::timer::timg::TimerGroup;
use esp_println::dbg;
use esp_println::println;
use firmware::audio::I2sConfig;
use firmware::codec::I2sSimplex;
use firmware::codec::I2sSimplexConfig;
use firmware::mk_buf;
use firmware::net::TlsClient;
use firmware::net::WebSocketClient;
use firmware::proto::BufTransport;
use firmware::proto::Transport;
use firmware::wifi::{WifiConfig, WifiConnection};
use firmware::DummyAudio;
use firmware::Robot;
use firmware::RobotState;
use log::info;

const TCP_BUF_SIZE: usize = 1024;

#[esp_hal_embassy::main]
async fn main(s: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);
    esp_alloc::heap_allocator!(size: 150 * 1024);
    esp_println::logger::init_logger_from_env();
    let stack = {
        let cfg = WifiConfig {
            ssid: "Wokwi-GUEST",
            password: None,
            wifi: peripherals.WIFI,
            timg: peripherals.TIMG0,
            rng: unsafe { peripherals.RNG.clone_unchecked() },
            radio_clk: peripherals.RADIO_CLK,
        };
        let stack = WifiConnection::connect(s, cfg).await;
        info!("Waiting for IP");
        stack.wait_config_up().await;
        let ip = stack.config_v4().unwrap().address;
        info!("Got IP: {}", ip);
        stack
    };

    // let mqtt = {
    //     info!("Connecting to Wifi");
    //     MqttUdp::build(s, stack, "172.20.10.8:8080".parse().unwrap()).await
    // };

    info!("Connecting to WebSocket");
    let state = TcpClientState::new();
    let tcp = TcpClient::<1, TCP_BUF_SIZE, TCP_BUF_SIZE>::new(stack, &state);
    let dns = DnsSocket::new(stack);
    let tls = TlsClient::new(
        tcp,
        dns,
        Trng::new(peripherals.RNG, peripherals.ADC1),
        mk_buf!(4096),
        mk_buf!(1024),
    );
    let mut ws = WebSocketClient::new(tls, EmptyRng::new(), mk_buf!(1024), mk_buf!(1024));
    let mut conn = ws
        .connect(
            "https://echo.websocket.org",
            Some(&["Device-Id:E4:59:76:78:E0:24"]),
        )
        .await
        .unwrap()
        .into_buffered(1024);
    conn.buf_read().await.unwrap();

    // let codec = {
    //     let (speaker_buf, speaker_tx) = I2sConfig {
    //         i2s: peripherals.I2S0,
    //         dma: peripherals.DMA_CH0,
    //         bclk: peripherals.GPIO15,
    //         ws: peripherals.GPIO16,
    //     }
    //     .build_output(peripherals.GPIO7);
    //     let (mic_buf, mic_rx) = I2sConfig {
    //         i2s: peripherals.I2S1,
    //         dma: peripherals.DMA_CH1,
    //         ws: peripherals.GPIO4,
    //         bclk: peripherals.GPIO5,
    //     }
    //     .build_input(peripherals.GPIO6);
    //     I2sSimplex::new(
    //         &s,
    //         I2sSimplexConfig {
    //             mic_rx,
    //             mic_buf,
    //             speaker_tx,
    //             speaker_buf,
    //         },
    //     )
    // };

    let mut robot = Robot::new(conn, DummyAudio);
    robot.set_state(RobotState::Idle).await;
    robot.main_loop().await;
}
