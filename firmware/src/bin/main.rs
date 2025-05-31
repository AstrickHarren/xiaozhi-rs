#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]
#![feature(inherent_str_constructors)]

use core::cell::RefCell;
use core::net::SocketAddr;
use core::ops::DerefMut;

use embassy_executor::Spawner;
use embassy_net::dns::DnsQueryType;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::TcpClient;
use embassy_net::tcp::client::TcpClientState;
use embassy_net::tcp::client::TcpConnection;
use embassy_net::IpEndpoint;
use embassy_net::Stack;
use embedded_io_async::Read;
use embedded_io_async::Write;
use embedded_nal_async::TcpConnect;
use embedded_tls::Aes128GcmSha256;
use embedded_tls::NoVerify;
use embedded_tls::TlsError;
use embedded_tls::{TlsConfig, TlsConnection, TlsContext};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::peripheral::Peripheral;
use esp_hal::rng::Rng;
use esp_hal::rng::Trng;
use esp_hal::timer::timg::TimerGroup;
use esp_println::dbg;
use esp_println::println;
use firmware::audio::I2sConfig;
use firmware::codec::I2sSimplex;
use firmware::codec::I2sSimplexConfig;
use firmware::mk_buf;
use firmware::mk_static;
use firmware::proto::MqttUdp;
use firmware::wifi::{WifiConfig, WifiConnection};
use firmware::Robot;
use firmware::RobotState;
use log::debug;
use log::info;
use reqwless::client::HttpClient;
use reqwless::client::TlsVerify;
use reqwless::request::Method;
use reqwless::request::RequestBuilder;
use rust_mqtt::utils::rng_generator::CountingRng;

const TCP_BUF_SIZE: usize = 1024;
const TCP_QUEUE_SIZE: usize = 3;
const MQTT_MAX_PROPERTIES: usize = 5;
const UDP_BUF_SIZE: usize = 512;

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

    // let proto = {
    //     info!("Connecting to Wifi");
    //     MqttUdp::build(s, stack, "172.20.10.8:8080".parse().unwrap()).await
    // };

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

    // tls

    let remote: IpEndpoint = "10.13.37.2:443".parse().unwrap();
    let state = &*mk_static!(
        TcpClientState::<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        TcpClientState::new()
    );
    let tcp = mk_static!(
        TcpClient::<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        TcpClient::new(stack, state)
    );
    let rx = mk_buf![u8, 0; TCP_BUF_SIZE];
    const KEEP_ALIVE: u16 = 60;

    const URL: &'static str = "https://host.wokwi.internal";
    let tls = TlsClient {
        tcp,
        rng: Trng::new(peripherals.RNG, peripherals.ADC1).into(),
    };
    let dns = DnsSocket::new(stack);
    let mut http = HttpClient::new(&tls, &dns);
    let mut req = http.request(Method::GET, URL).await.unwrap();
    let mut resp = req.send(rx).await.unwrap().body().reader();
    let body = &mut [0; 1024];
    let n = resp.read(body).await.unwrap();
    dbg!(str::from_utf8(&body[..n]));

    // println!("connecting to {URL}");
    // let ip = dns
    //     .query("host.wokwi.internal.come", DnsQueryType::A)
    //     .await
    //     .unwrap()
    //     .first()
    //     .unwrap();
    // debug!("tcp connecting to {}", remote);
    // let tcp_conn = tcp
    //     .connect(SocketAddr::new(remote.addr.into(), 1883))
    //     .await
    //     .unwrap();
    // let mut tls = TlsConnection::new(tcp_conn, rx, tx);
    // let mut trng = Trng::new(peripherals.RNG, peripherals.ADC1);
    // let config = TlsConfig::<Aes128GcmSha256>::new().with_server_name("localhost");
    // tls.open::<_, embedded_tls::NoVerify>(TlsContext::new(&config, &mut trng))
    //     .await
    //     .expect("error establishing TLS connection");
    // HttpClient::new(&tls, &dns);
    debug!("tcp connected to {}", remote);

    // tls.write_all(b"GET /path/resource HTTP/1.1\r\n Host: example.com\r\n ")
    //     .await
    //     .expect("error writing data");
    // tls.flush().await.expect("error flushing data");
    // let mut rx_buf = [0; 128];
    // let sz = tls.read(&mut rx_buf[..]).await.expect("error reading data");
    // log::info!("Read {} bytes: {:?}", sz, &rx_buf[..sz]);

    // let mut robot = Robot::new(proto, codec);
    // robot.set_state(RobotState::Idle).await;
    // robot.main_loop().await;
}

struct TlsClient<'a> {
    tcp: &'a mut TcpClient<'a, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
    rng: RefCell<Trng<'a>>,
}

impl<'b> TcpConnect for TlsClient<'b> {
    type Error = TlsError;

    type Connection<'a>
        = TlsConnection<
        'a,
        TcpConnection<'a, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        Aes128GcmSha256,
    >
    where
        Self: 'a;

    async fn connect<'a>(
        &'a self,
        remote: SocketAddr,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        let rx = mk_buf![u8, 0; TCP_BUF_SIZE];
        let tx = mk_buf![u8, 0; 512];
        const KEEP_ALIVE: u16 = 60;

        const URL: &'static str = "https://google.com";
        println!("connecting to {URL}");
        let tcp_conn = self.tcp.connect(remote).await.unwrap();
        let mut tls = TlsConnection::new(tcp_conn, rx, tx);
        let config = TlsConfig::<Aes128GcmSha256>::new().with_server_name("localhost");
        tls.open::<_, embedded_tls::NoVerify>(TlsContext::new(
            &config,
            self.rng.borrow_mut().deref_mut(),
        ))
        .await
        .expect("error establishing TLS connection");
        Ok(tls)
    }
}
