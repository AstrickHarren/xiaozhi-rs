use embassy_executor::Spawner;
use embassy_net::{Runner, Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::{peripherals::*, rng::Rng, timer::timg::TimerGroup};
use esp_wifi::wifi::AuthMethod;
use esp_wifi::{
    init,
    wifi::{ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiState},
    EspWifiController,
};
use log::{debug, error, info};

pub struct WifiConfig {
    pub ssid: &'static str,
    pub password: Option<&'static str>,

    pub wifi: WIFI,
    pub timg: TIMG0,
    pub rng: RNG,
    pub radio_clk: RADIO_CLK,
}

pub struct WifiConnection {
    pub controller: WifiController<'static>,
}

// When you are okay with using a nightly compiler, it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

impl WifiConnection {
    pub async fn connect(spawner: Spawner, cfg: WifiConfig) -> Stack<'static> {
        let mut rng = Rng::new(cfg.rng);
        let seed = (rng.random() as u64) << 32 | rng.random() as u64;
        let init = &*mk_static!(
            EspWifiController<'static>,
            init(TimerGroup::new(cfg.timg).timer0, rng, cfg.radio_clk).unwrap()
        );

        // Init network stack
        let (controller, ifaces) = esp_wifi::wifi::new(&init, cfg.wifi).unwrap();
        let (stack, runner) = embassy_net::new(
            ifaces.sta,
            embassy_net::Config::dhcpv4(Default::default()),
            mk_static!(StackResources<3>, StackResources::<3>::new()),
            seed,
        );

        spawner
            .spawn(net_task(runner))
            .inspect_err(|e| esp_println::println!("Error spawning net_task: {:?}", e))
            .ok();
        spawner
            .spawn(connection(
                controller,
                cfg.password
                    .map(|_| AuthMethod::default())
                    .unwrap_or_else(|| AuthMethod::None),
                cfg.ssid,
                cfg.password.unwrap_or_default(),
            ))
            .ok();

        stack.wait_link_up().await;
        stack
    }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, WifiDevice<'static>>) {
    runner.run().await
}

#[embassy_executor::task]
async fn connection(
    mut controller: WifiController<'static>,
    auth_method: AuthMethod,
    ssid: &'static str,
    password: &'static str,
) {
    debug!("start connection task");
    debug!("Device capabilities: {:?}", controller.capabilities());

    loop {
        match esp_wifi::wifi::wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                auth_method,
                ssid: ssid.try_into().unwrap(),
                password: password.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            debug!("Starting wifi");
            controller.start_async().await.unwrap();
            debug!("Wifi started!");
        }

        info!("Connecting to {}", ssid);
        match controller.connect_async().await {
            Ok(_) => info!("Wifi connected!"),
            Err(e) => {
                error!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}
