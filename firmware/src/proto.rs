use core::{
    cell::RefCell,
    future::{self, pending},
    net::SocketAddr,
    slice,
};

use bytes::{BufMut, BytesMut};
use embassy_net::{
    tcp::client::{TcpClient, TcpClientState, TcpConnection},
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use embedded_nal_async::TcpConnect;
use esp_println::{dbg, println};
use log::{debug, error, info};
use rust_mqtt::{
    client::{
        client::MqttClient,
        client_config::{ClientConfig, MqttVersion},
    },
    utils::rng_generator::CountingRng,
};
use serde::{Deserialize, Serialize};

use crate::{Command, Protocol};

const TCP_BUF_SIZE: usize = 512;
const TCP_QUEUE_SIZE: usize = 3;
const MQTT_MAX_PROPERTIES: usize = 5;
const UDP_BUF_SIZE: usize = 512;

pub struct MqttUdp {
    socket: UdpSocket<'static>,
    mqtt: RefCell<
        MqttClient<
            'static,
            TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
            MQTT_MAX_PROPERTIES,
            CountingRng,
        >,
    >,
    remote: IpEndpoint,
}

impl MqttUdp {
    pub async fn build(stack: Stack<'static>, remote: IpEndpoint) -> Self {
        // Start MQTT
        let mut mqtt = Self::connect_mqtt(stack, remote).await;
        mqtt.connect_to_broker().await.unwrap();
        mqtt.subscribe_to_topic("ai/chatbot").await.unwrap();

        // Start UDP
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

        Self {
            socket: udp,
            mqtt: mqtt.into(),
            remote,
        }
    }

    async fn connect_mqtt(
        stack: Stack<'static>,
        remote: IpEndpoint,
    ) -> MqttClient<
        'static,
        TcpConnection<'static, TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
        MQTT_MAX_PROPERTIES,
        CountingRng,
    > {
        let state = &*mk_static!(
            TcpClientState::<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
            TcpClientState::new()
        );
        debug!("tcp connection to MQTT broker at {}", remote);
        let connection = mk_static!(
                TcpClient<TCP_QUEUE_SIZE, TCP_BUF_SIZE, TCP_BUF_SIZE>,
                TcpClient::new(stack, state))
        .connect(SocketAddr::new(remote.addr.into(), 1883))
        .await
        .unwrap();
        let mut mqtt_client_config =
            ClientConfig::<MQTT_MAX_PROPERTIES, _>::new(MqttVersion::MQTTv5, CountingRng(12345));
        mqtt_client_config.add_client_id("oidfsduidiodsuio");
        mqtt_client_config.add_username("alice");
        mqtt_client_config.add_password("123");

        MqttClient::new(
            connection,
            mk_buf![u8, 0; TCP_BUF_SIZE],
            TCP_BUF_SIZE,
            mk_buf![u8, 0; TCP_BUF_SIZE],
            TCP_BUF_SIZE,
            mqtt_client_config,
        )
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
            match self.mqtt.borrow_mut().receive_message().await {
                Ok((_, payload)) => {
                    let (msg, _) = serde_json_core::from_slice::<Msg>(payload).unwrap();
                    debug!("mqtt: received msg {:?}", msg);
                    break Ok(msg.command);
                }
                Err(e) => {
                    error!("mqtt: {e:?}");
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
