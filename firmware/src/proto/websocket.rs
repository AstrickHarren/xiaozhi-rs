use bytes::Bytes;
use core::marker::PhantomData;
use embassy_net::tcp::client::TcpConnection;
use embedded_tls::{Aes128GcmSha256, TlsConnection, TlsError};
use embedded_websocket::{
    framer_embedded::{Framer, FramerError, ReadResult},
    Client, EmptyRng, WebSocketOptions,
};
use esp_println::println;
use rand_core::RngCore;

use crate::{Command, Msg, Protocol};

const TCP_Q_SZ: usize = 3;
const TCP_RX_SZ: usize = 1024;
const TCP_TX_SZ: usize = 1024;
type TcpConn<'a> = TcpConnection<'a, TCP_Q_SZ, TCP_RX_SZ, TCP_TX_SZ>;

type TlsConn<'a> = TlsConnection<'a, TcpConn<'a>, Aes128GcmSha256>;

pub struct WebSocket<'a, R>
where
    R: RngCore,
{
    framer: Framer<'a, R, Client>,
    conn: TlsConn<'a>,
}

impl<'a, R> WebSocket<'a, R>
where
    R: RngCore,
{
    pub async fn new(
        ws: &'a mut embedded_websocket::WebSocketClient<R>,
        conn: TlsConn<'a>,
    ) -> Self {
        let framer = Framer::new(
            mk_buf![ u8, 0; 10224 ],
            mk_static!(usize, 0),
            mk_buf![ u8, 0; 1024 ],
            ws,
        );
        Self { framer, conn }
    }

    pub async fn connect(
        &mut self,
        opts: WebSocketOptions<'_>,
    ) -> Result<(), FramerError<TlsError>> {
        self.framer.connect(&mut self.conn, &opts).await?;
        Ok(())
    }
}

impl<'a, R> WebSocket<'a, R>
where
    R: RngCore,
{
    pub async fn send_text(&mut self, text: &str) -> Result<(), FramerError<TlsError>> {
        self.framer
            .write(
                &mut self.conn,
                embedded_websocket::WebSocketSendMessageType::Text,
                true,
                text.as_bytes(),
            )
            .await
    }
}

impl<'a, R> Protocol for WebSocket<'a, R>
where
    R: RngCore,
{
    type Error = FramerError<TlsError>;
    async fn send_bin(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.framer
            .write(
                &mut self.conn,
                embedded_websocket::WebSocketSendMessageType::Binary,
                true,
                data,
            )
            .await
    }

    async fn recv(&mut self) -> Result<crate::Msg, Self::Error> {
        let mut buf = [0; 1024];
        loop {
            let result = self.framer.read(&mut self.conn, &mut buf).await?;
            match result {
                ReadResult::Binary(bytes) => break Ok(Msg::Audio(Bytes::copy_from_slice(&bytes))),
                ReadResult::Text(t) => {
                    println!("recved text {}", t);
                    break Ok(Msg::Cmd(Command::Stop));
                }
                ReadResult::Closed => break Err(FramerError::Io(TlsError::ConnectionClosed)),
                _ => (),
            }
        }
    }
}
