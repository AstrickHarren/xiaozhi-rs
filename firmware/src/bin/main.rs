#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use firmware::audio::I2sConfig;
use firmware::codec::I2sSimplex;
use firmware::codec::I2sSimplexConfig;
use firmware::proto::MqttUdp;
use firmware::wifi::{WifiConfig, WifiConnection};
use firmware::Robot;
use firmware::RobotState;
use log::info;
use log::warn;
use log::LevelFilter;

#[esp_hal_embassy::main]
async fn main(s: Spawner) {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    esp_hal_embassy::init(timg1.timer0);
    esp_alloc::heap_allocator!(size: 150 * 1024);
    esp_println::logger::init_logger_from_env();

    let proto = {
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
        MqttUdp::build(stack, "172.20.10.8:8080".parse().unwrap()).await
    };

    let codec = {
        let (speaker_buf, speaker_tx) = I2sConfig {
            i2s: peripherals.I2S0,
            dma: peripherals.DMA_CH0,
            bclk: peripherals.GPIO15,
            ws: peripherals.GPIO16,
        }
        .build_output(peripherals.GPIO7);
        let (mic_buf, mic_rx) = I2sConfig {
            i2s: peripherals.I2S1,
            dma: peripherals.DMA_CH1,
            ws: peripherals.GPIO4,
            bclk: peripherals.GPIO5,
        }
        .build_input(peripherals.GPIO6);
        I2sSimplex::new(
            &s,
            I2sSimplexConfig {
                mic_rx,
                mic_buf,
                speaker_tx,
                speaker_buf,
            },
        )
    };

    let mut robot = Robot::new(proto, codec);
    robot.set_state(RobotState::Listening).await;
    robot.main_loop().await;
}
