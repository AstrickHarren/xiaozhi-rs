use core::net::SocketAddr;

use bytes::{BufMut, BytesMut};
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_net::{
    tcp::client::{TcpClient, TcpClientState, TcpConnection},
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Receiver, Sender},
};
use embassy_time::Timer;
use embedded_nal_async::TcpConnect;
use log::{debug, error, info, warn};
use rust_mqtt::{
    client::client_config::{ClientConfig, MqttVersion},
    utils::rng_generator::CountingRng,
};
use serde::{Deserialize, Serialize};

use crate::util::{BytesMutExtend, SliceExt};

const TCP_BUF_SIZE: usize = 512;
const TCP_QUEUE_SIZE: usize = 3;
const MQTT_MAX_PROPERTIES: usize = 5;
const UDP_BUF_SIZE: usize = 512;

type Mutex<T> = embassy_sync::mutex::Mutex<NoopRawMutex, T>;
type MqttClient = rust_mqtt::client::client::MqttClient<
    'static,
    TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
    MQTT_MAX_PROPERTIES,
    CountingRng,
>;

pub struct MqttUdp {
    stack: Stack<'static>,
    socket: UdpSocket<'static>,
    mqtt: &'static Mutex<Option<MqttClient>>,
    mqtt_connected: Receiver<'static, NoopRawMutex, (), 1>,
    mqtt_reconnect: Sender<'static, NoopRawMutex, (), 1>,
    mqtt_need_ping: Receiver<'static, NoopRawMutex, (), 1>,
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
        let (needping_tx, needping_rx) = mk_ch!(1; ());
        let this = Self {
            stack,
            socket: udp,
            mqtt: mk_static!(Mutex<Option<MqttClient>>, Mutex::new(None)),
            mqtt_reconnect: reconnect_tx,
            mqtt_connected: connected_rx,
            mqtt_need_ping: needping_rx,
            remote,
        };
        this.connect_mqtt(spawner, connected_tx, reconnect_rx, needping_tx)
            .await;
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
        needping: Sender<'static, NoopRawMutex, (), 1>,
    ) {
        spawner
            .spawn(task(
                connected,
                reconnect,
                needping,
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
    needping: Sender<'static, NoopRawMutex, (), 1>,
    mqtt: &'static Mutex<Option<MqttClient>>,
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
    const KEEP_ALIVE: u16 = 60;
    let config = || {
        let mut c =
            ClientConfig::<MQTT_MAX_PROPERTIES, _>::new(MqttVersion::MQTTv5, CountingRng(12345));
        c.add_client_id("oidfsduidiodsuio");
        c.add_username("alice");
        c.add_password("123");
        c.keep_alive = KEEP_ALIVE;
        c
    };

    loop {
        debug!("tcp connecting to {}", remote);
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
                    unsafe { rx.force_mut() },
                    TCP_BUF_SIZE,
                    unsafe { tx.force_mut() },
                    TCP_BUF_SIZE,
                    config(),
                );
                debug!("mqtt connecting to {}", remote);
                mqtt.connect_to_broker().await.unwrap();
                mqtt.subscribe_to_topic("ai/chatbot").await.unwrap();
                Some(mqtt)
            }
        }
        info!("mqtt connected to {}", remote);
        connected.send(()).await;

        use embassy_futures::select::Either::*;

        while let First(_) = select(
            Timer::after_secs((KEEP_ALIVE / 2) as u64),
            reconnect.receive(),
        )
        .await
        {
            needping.send(()).await
        }
    }
}

// #[derive(Debug, Serialize, Deserialize)]
// struct MqttMsg {
//     command: Command,
// }

// impl MqttUdp {
//     async fn recv_cmd(&self) -> Result<crate::Command, ()> {
//         use embassy_futures::select::Either::*;
//         loop {
//             let mut mqtt = self.mqtt.lock().await;
//             match select(
//                 mqtt.as_mut().unwrap().receive_message(),
//                 self.mqtt_need_ping.receive(),
//             )
//             .await
//             {
//                 First(Ok((_, payload))) => {
//                     let (msg, _) = serde_json_core::from_slice::<MqttMsg>(payload).unwrap();
//                     debug!("mqtt: received msg {:?}", msg);
//                     break Ok(msg.command);
//                 }
//                 First(Err(e)) => {
//                     drop(mqtt);
//                     warn!("Mqtt disconnected because {e:?}, reconnecting");
//                     self.reconnect().await;
//                 }
//                 Second(_) => {
//                     debug!("Mqtt send ping");
//                     mqtt.as_mut().unwrap().send_ping().await.unwrap();
//                 }
//             }
//         }
//     }

//     async fn recv_bin(&self) -> Result<bytes::BytesMut, ()> {
//         let mut buf = BytesMut::with_capacity(1024);
//         let (n, _) = self.socket.recv_from(buf.transmute_cap()).await.unwrap();
//         unsafe { buf.advance_mut(n) };
//         Ok(buf)
//     }
// }

// // impl Protocol for MqttUdp {
// //     type Error = ();

// //     async fn recv(&mut self) -> Result<crate::Msg, Self::Error> {
// //         use embassy_futures::select::Either::*;
// //         let msg = match select(self.recv_bin(), self.recv_cmd()).await {
// //             First(bin) => Msg::Audio(bin?.freeze()),
// //             Second(cmd) => Msg::Cmd(cmd?),
// //         };
// //         Ok(msg)
// //     }

// //     async fn send_bin(&mut self, data: &[u8]) -> Result<(), Self::Error> {
// //         Ok(self.socket.send_to(data, self.remote).await.unwrap())
// //     }
// // }
