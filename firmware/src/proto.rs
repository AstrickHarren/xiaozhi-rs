use core::{
    cell::RefCell,
    future::{self, pending},
    net::SocketAddr,
    ops::Deref,
    ptr::slice_from_raw_parts_mut,
    slice,
};

use bytes::{BufMut, BytesMut};
use embassy_executor::Spawner;
use embassy_net::{
    tcp::client::{TcpClient, TcpClientState, TcpConnection},
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Receiver, Sender},
    mutex::Mutex,
};
use embassy_time::Timer;
use embedded_nal_async::TcpConnect;
use esp_println::{dbg, println};
use log::{debug, error, info, warn};
use rust_mqtt::{
    client::{
        client::MqttClient,
        client_config::{ClientConfig, MqttVersion},
    },
    packet::v5::reason_codes::ReasonCode,
    utils::rng_generator::CountingRng,
};
use serde::{Deserialize, Serialize};
use static_cell::make_static;

use crate::{Command, Protocol};

const TCP_BUF_SIZE: usize = 512;
const TCP_QUEUE_SIZE: usize = 3;
const MQTT_MAX_PROPERTIES: usize = 5;
const UDP_BUF_SIZE: usize = 512;

pub struct MqttUdp {
    stack: Stack<'static>,
    socket: UdpSocket<'static>,
    mqtt: &'static Mutex<
        NoopRawMutex,
        Option<
            MqttClient<
                'static,
                TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
                MQTT_MAX_PROPERTIES,
                CountingRng,
            >,
        >,
    >,
    mqtt_connected: Receiver<'static, NoopRawMutex, (), 1>,
    mqtt_reconnect: Sender<'static, NoopRawMutex, (), 1>,
    remote: IpEndpoint,
}

impl MqttUdp {
    pub async fn build(spawner: Spawner, stack: Stack<'static>, remote: IpEndpoint) -> Self {
        let (rx, tx, rx_meta, tx_meta) = (
            mk_buf![ u8, 0; UDP_BUF_SIZE ],
            mk_buf![ u8, 0; UDP_BUF_SIZE ],
            mk_buf![ PacketMetadata, PacketMetadata::EMPTY; 10 ],
            mk_buf![ PacketMetadata, PacketMetadata::EMPTY; 10 ],
        );
        let mut udp = UdpSocket::new(stack, rx_meta, rx, tx_meta, tx);
        let addr = stack.config_v4().unwrap().address.address();
        udp.bind(IpEndpoint::new(addr.into(), 8080))
            .inspect_err(|e| error!("Failed to bind UDP socket: {e:?}"))
            .ok();

        let (reconnect_tx, reconnect_rx) = mk_ch!(1; ());
        let (connected_tx, connected_rx) = mk_ch!(1; ());
        let this = Self {
            stack,
            socket: udp,
            mqtt: mk_static!(Mutex<
                NoopRawMutex,
                Option<
                    MqttClient<
                        'static,
                        TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
                        MQTT_MAX_PROPERTIES,
                        CountingRng,
                    >,
                >,
            >, Mutex::new(None)),
            mqtt_reconnect: reconnect_tx,
            mqtt_connected: connected_rx,
            remote,
        };
        this.connect_mqtt(spawner, connected_tx, reconnect_rx).await;
        this
    }

    async fn reconnect(&self) {
        self.mqtt_reconnect.send(()).await;
        drop(self.mqtt.lock().await.as_mut().take());
        self.mqtt_connected.receive().await;
        debug!("reconnect: received connected");
    }

    async fn connect_mqtt(
        &self,
        spawner: Spawner,
        connected: Sender<'static, NoopRawMutex, (), 1>,
        reconnect: Receiver<'static, NoopRawMutex, (), 1>,
    ) {
        spawner
            .spawn(task(
                connected,
                reconnect,
                self.mqtt,
                self.stack,
                self.remote,
            ))
            .unwrap();
        self.mqtt_connected.receive().await
    }
}

#[embassy_executor::task]
async fn task(
    connected: Sender<'static, NoopRawMutex, (), 1>,
    reconnect: Receiver<'static, NoopRawMutex, (), 1>,
    mqtt: &'static Mutex<
        NoopRawMutex,
        Option<
            MqttClient<
                'static,
                TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
                MQTT_MAX_PROPERTIES,
                CountingRng,
            >,
        >,
    >,
    stack: Stack<'static>,
    remote: IpEndpoint,
) {
    let state = &*mk_static!(
        TcpClientState::<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        TcpClientState::new()
    );
    let tcp = mk_static!(
        TcpClient::<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        TcpClient::new(stack, state)
    );
    let rx = mk_buf![u8, 0; TCP_BUF_SIZE];
    let tx = mk_buf![u8, 0; TCP_BUF_SIZE];
    let config = || {
        let mut c =
            ClientConfig::<MQTT_MAX_PROPERTIES, _>::new(MqttVersion::MQTTv5, CountingRng(12345));
        c.add_client_id("oidfsduidiodsuio");
        c.add_username("alice");
        c.add_password("123");
        c.keep_alive = 10;
        c
    };

    loop {
        debug!("mqtt connecting to {}", remote);
        let connection = tcp
            .connect(SocketAddr::new(remote.addr.into(), 1883))
            .await
            .unwrap();
        debug!("tcp connected to {}", remote);
        {
            let mut mqtt = mqtt.lock().await;
            drop(mqtt.take());
            *mqtt = {
                let mut mqtt = MqttClient::new(
                    connection,
                    unsafe { slice::from_raw_parts_mut(rx.as_ptr() as *mut u8, rx.len()) },
                    TCP_BUF_SIZE,
                    unsafe { slice::from_raw_parts_mut(tx.as_ptr() as *mut u8, tx.len()) },
                    TCP_BUF_SIZE,
                    config(),
                );
                debug!("mqtt connected to {}", remote);
                mqtt.connect_to_broker().await.unwrap();
                mqtt.subscribe_to_topic("ai/chatbot").await.unwrap();
                Some(mqtt)
            }
        }
        info!("mqtt connected to {}", remote);

        connected.send(()).await;
        reconnect.receive().await
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Msg {
    command: Command,
}

impl Protocol for MqttUdp {
    type Error = ();

    async fn recv_cmd(&self) -> Result<crate::Command, Self::Error> {
        loop {
            let mut mqtt = self.mqtt.lock().await;
            match mqtt.as_mut().unwrap().receive_message().await {
                Ok((_, payload)) => {
                    let (msg, _) = serde_json_core::from_slice::<Msg>(payload).unwrap();
                    debug!("mqtt: received msg {:?}", msg);
                    break Ok(msg.command);
                }
                Err(e) => {
                    drop(mqtt);
                    // warn!("Mqtt disconnected because {e:?}, reconnecting");
                    self.reconnect().await;
                    println!("reconnect returned");
                }
            }
        }
    }

    async fn send_bin(&self, data: &[u8]) -> Result<(), Self::Error> {
        Ok(self.socket.send_to(data, self.remote).await.unwrap())
    }

    async fn recv_bin(&self) -> Result<bytes::BytesMut, Self::Error> {
        let mut buf = BytesMut::with_capacity(1024);
        let (n, _) = self
            .socket
            .recv_from(unsafe { slice::from_raw_parts_mut(buf.as_mut_ptr(), 1024) })
            .await
            .unwrap();
        unsafe { buf.advance_mut(n) };
        Ok(buf)
    }
}

pub struct Mqtt {
    tcp: RefCell<TcpClient<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>>,
    mqtt: MqttClient<
        'static,
        TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        MQTT_MAX_PROPERTIES,
        CountingRng,
    >,
}

impl Mqtt {}
