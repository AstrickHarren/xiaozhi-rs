use core::{future, slice};

use bytes::{BufMut, BytesMut};
use embassy_net::{
    udp::{PacketMetadata, UdpSocket},
    IpEndpoint, Stack,
};
use log::error;

use crate::Protocol;

// When you are okay with using a nightly compiler, it's better to use https://docs.rs/static_cell/2.1.0/static_cell/macro.make_static.html
macro_rules! mk_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        #[deny(unused_attributes)]
        let x = STATIC_CELL.uninit().write(($val));
        x
    }};
}

macro_rules! mk_buf {
    [ $ty:ty , $filler:expr  ;$size:expr ] => {
        mk_static!([$ty; $size], [$filler; $size])
    };
}

pub struct MqttUdp {
    socket: UdpSocket<'static>,
    remote: IpEndpoint,
}

impl MqttUdp {
    pub fn build(stack: Stack<'static>, remote: IpEndpoint) -> Self {
        const UDP_BUF_SIZE: usize = 4096;
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
            remote,
        }
    }
}

impl Protocol for MqttUdp {
    type Error = ();

    async fn recv_cmd(&self) -> Result<crate::Command, Self::Error> {
        future::pending().await
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
