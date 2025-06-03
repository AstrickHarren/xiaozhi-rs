use core::{
    cell::RefCell,
    fmt::{Debug, Display},
    future::Future,
};

use bytes::{Bytes, BytesMut};
use esp_println::println;
use serde::{Deserialize, Serialize};

use crate::util::BytesMutExtend;

pub mod mqtt_udp;

pub enum MsgType {
    Text,
    Binary,
}

#[derive(Debug)]
pub enum ProtoMsg<'a> {
    Text(&'a str),
    Binary(&'a [u8]),
}

impl<'a> Display for ProtoMsg<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProtoMsg::Text(text) => write!(f, "{}", text),
            ProtoMsg::Binary(data) => write!(f, "{:?}", data),
        }
    }
}

pub trait Transport {
    type Error: Debug;

    fn read<'a>(
        &mut self,
        buf: &'a mut [u8],
    ) -> impl Future<Output = Result<ProtoMsg<'a>, Self::Error>>;
    fn write(&mut self, msg: ProtoMsg) -> impl Future<Output = Result<(), Self::Error>>;

    fn send_bin(&mut self, data: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
        self.write(ProtoMsg::Binary(data))
    }

    fn send_text(&mut self, text: &str) -> impl Future<Output = Result<(), Self::Error>> {
        self.write(ProtoMsg::Text(text))
    }

    fn into_buffered(self, capacity: usize) -> Buffered<Self>
    where
        Self: Sized,
    {
        Buffered::with_capacity(self, capacity)
    }
}

pub trait BufTransport: Transport {
    fn buf_read(&mut self) -> impl Future<Output = Result<ProtoMsg<'_>, Self::Error>>;
}

pub struct Buffered<P> {
    inner: P,
    buf: BytesMut,
}

impl<P> Buffered<P> {
    fn with_capacity(inner: P, capacity: usize) -> Self {
        Self {
            inner,
            buf: BytesMut::with_capacity(capacity),
        }
    }
}

impl<P: Transport> Transport for Buffered<P> {
    type Error = P::Error;

    async fn read<'a>(&mut self, buf: &'a mut [u8]) -> Result<ProtoMsg<'a>, Self::Error> {
        self.inner.read(buf).await
    }

    async fn write(&mut self, msg: ProtoMsg<'_>) -> Result<(), Self::Error> {
        self.inner.write(msg).await
    }
}

impl<P: Transport> BufTransport for Buffered<P> {
    async fn buf_read(&mut self) -> Result<ProtoMsg<'_>, Self::Error> {
        self.inner.read(self.buf.transmute_cap()).await
    }
}

pub struct Protocol<T> {
    pub transport: T,
}

extern crate alloc;
use alloc::string::String;
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerText {
    Hello {},
    Stt { text: String },
    Llm { text: String },
    Tts(Tts),
    Iot {},
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Tts {
    Start,
    Stop,
    SentenceStart { text: String },
    SentenceEnd {},
}

pub enum ServerMsg<'a> {
    Unknown(&'a str),
    Text(ServerText),
    Binary(&'a [u8]),
}

impl<T: BufTransport> Protocol<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub async fn send_hello(&mut self) -> Result<(), T::Error> {
        let json = r#"
        {
          "type": "hello",
          "version": 1,
          "transport": "websocket",
          "audio_params": {
            "format": "opus",
            "sample_rate": 16000,
            "channels": 1,
            "frame_duration": 60
          }
        }"#;
        self.transport.write(ProtoMsg::Text(json)).await
    }

    pub async fn recv_hello(&mut self) -> Result<&str, T::Error> {
        #[derive(Deserialize)]
        struct ServerHello<'a> {
            session_id: &'a str,
        }

        match self.transport.buf_read().await? {
            ProtoMsg::Text(t) => {
                let (hello) = serde_json::from_str::<ServerHello>(&t).unwrap();
                Ok(hello.session_id)
            }
            ProtoMsg::Binary(_) => {
                panic!("Unexpected binary message")
            }
        }
    }

    pub async fn recv(&mut self) -> Result<ServerMsg, T::Error> {
        match self.transport.buf_read().await? {
            ProtoMsg::Text(t) => Ok(serde_json::from_str::<ServerText>(&t)
                .map(ServerMsg::Text)
                .inspect_err(|e| log::error!("{e}"))
                .unwrap_or_else(|_| ServerMsg::Unknown(t))),
            ProtoMsg::Binary(b) => Ok(ServerMsg::Binary(&b)),
        }
    }

    pub async fn send_listening(&mut self, session_id: &str) -> Result<(), T::Error> {
        extern crate alloc;
        let msg = alloc::format!(
            r#"{{
            "session_id": "{session_id}",
            "type": "listen",
            "state": "start",
            "mode": "auto"
            }}
            "#
        );
        self.transport.send_text(msg.as_str()).await
    }

    pub async fn send_listening_stop(&mut self, session_id: &str) -> Result<(), T::Error> {
        extern crate alloc;
        let msg = alloc::format!(
            r#"{{
            "session_id": "{session_id}",
            "type": "listen",
            "state": "stop",
            "mode": "auto"
            }}
            "#
        );
        self.transport.send_text(msg.as_str()).await
    }
}
