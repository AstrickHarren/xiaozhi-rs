use core::{fmt::Debug, future::Future, net::SocketAddr};

use embassy_net::tcp::client::{TcpClient, TcpConnection};
use embedded_io_async::{ErrorType, Read, Write};
use embedded_nal_async::{Dns, TcpConnect};
use embedded_tls::{
    Aes128GcmSha256, NoVerify, TlsConfig, TlsConnection, TlsContext, TlsError, UnsecureProvider,
};
use embedded_websocket::{
    framer_embedded::{Framer, FramerError, ReadResult},
    Client, WebSocketOptions, WebSocketSendMessageType,
};
use esp_hal::{
    peripheral::Peripheral,
    peripherals::{RSA, SHA},
};
use esp_mbedtls::{asynch::Session, Certificates, Mode, Tls, TlsVersion, X509};

use log::debug;
use rand_core_legacy::{CryptoRng, RngCore};

use crate::proto::{ProtoMsg, Transport};

pub trait Connect {
    type Remote: ?Sized;
    type Error: Debug;
    type Connection<'a>: Read + Write
    where
        Self: 'a;
    fn connect(
        &mut self,
        remote: &Self::Remote,
    ) -> impl Future<Output = Result<Self::Connection<'_>, Self::Error>>;
}

impl<'a, const N: usize, const TX: usize, const RX: usize> Connect for TcpClient<'a, N, TX, RX> {
    type Remote = SocketAddr;
    type Error = embassy_net::tcp::Error;
    type Connection<'b>
        = TcpConnection<'b, N, TX, RX>
    where
        Self: 'b;

    async fn connect(
        &mut self,
        remote: &Self::Remote,
    ) -> Result<Self::Connection<'_>, Self::Error> {
        let conn = <Self as TcpConnect>::connect(self, *remote).await?;
        debug!("tcp connection established");
        Ok(conn)
    }
}

pub struct TlsClient<'a, T, D, R> {
    tcp: T,
    dns: D,
    rng: R,
    rx_buf: &'a mut [u8],
    tx_buf: &'a mut [u8],
}

impl<'a, T, D, R> TlsClient<'a, T, D, R> {
    pub fn new(tcp: T, dns: D, rng: R, rx_buf: &'a mut [u8], tx_buf: &'a mut [u8]) -> Self {
        Self {
            tcp,
            dns,
            rng,
            rx_buf,
            tx_buf,
        }
    }
}

impl<'a, T, D, R> Connect for TlsClient<'a, T, D, R>
where
    T: Connect<Remote = SocketAddr>,
    D: Dns,
    R: CryptoRng + RngCore,
{
    type Remote = str;
    type Error = TlsError;
    type Connection<'b>
        = TlsConnection<'b, T::Connection<'b>, Aes128GcmSha256>
    where
        Self: 'b;

    async fn connect(&mut self, remote: &str) -> Result<Self::Connection<'_>, Self::Error> {
        let url = nourl::Url::parse(remote).unwrap();
        let ip = self
            .dns
            .get_host_by_name(url.host(), embedded_nal_async::AddrType::IPv4)
            .await
            .unwrap();
        let addr = SocketAddr::new(ip, url.port().unwrap_or(443));
        let tcp = self.tcp.connect(&addr).await.unwrap();
        let mut tls = TlsConnection::<_, Aes128GcmSha256>::new(tcp, self.rx_buf, self.tx_buf);
        let config = TlsConfig::new().with_server_name(url.host());
        let context = TlsContext::new(&config, UnsecureProvider::new(&mut self.rng));
        tls.open(context).await?;
        debug!("tls connection established");
        Ok(tls)
    }
}

pub struct EspTlsClient<'a, T, D> {
    tcp: T,
    tls: Tls<'a>,
    dns: D,
}

impl<'b, T, D> EspTlsClient<'b, T, D> {
    pub fn new<R, S>(tcp: T, dns: D, sha: S, rsa: R) -> Self
    where
        R: Peripheral<P = RSA> + 'b,
        S: Peripheral<P = SHA> + 'b,
    {
        Self {
            tcp,
            tls: Tls::new(sha).unwrap().with_hardware_rsa(rsa),
            dns,
        }
    }
}

impl<'b, T, D> Connect for EspTlsClient<'b, T, D>
where
    T: Connect<Remote = SocketAddr>,
    D: Dns,
{
    type Remote = str;

    type Error = T::Error;

    type Connection<'a>
        = Session<'a, T::Connection<'a>>
    where
        Self: 'a;

    async fn connect(
        &mut self,
        remote: &Self::Remote,
    ) -> Result<Self::Connection<'_>, Self::Error> {
        let url = nourl::Url::parse(remote).unwrap();
        let ip = self
            .dns
            .get_host_by_name(url.host(), embedded_nal_async::AddrType::IPv4)
            .await
            .unwrap();
        let addr = SocketAddr::new(ip, url.port().unwrap_or(443));

        let mut host = [0; 100];
        let bytes = url.host().as_bytes();
        host[..bytes.len()].copy_from_slice(bytes);
        let host = CStr::from_bytes_with_nul(&host[..bytes.len() + 1]).expect("unable to get host");

        // let certificates = Certificates {
        //     ca_chain: X509::pem(
        //         concat!(include_str!("./certs/www.google.com.pem"), "\0").as_bytes(),
        //     )
        //     .ok(),
        //     ..Default::default()
        // };

        use core::ffi::CStr;
        let mut session = Session::new(
            self.tcp.connect(&addr).await.unwrap(),
            Mode::Client { servername: host },
            TlsVersion::Tls1_3,
            Certificates::new(),
            self.tls.reference(),
        )
        .unwrap();

        session.connect().await.unwrap();
        debug!("tls connection established");
        Ok(session)
    }
}

pub struct WebSocketClient<'a, T, R: rand_core::RngCore> {
    tcp: T,
    ws: embedded_websocket::WebSocketClient<R>,
    tx_buf: &'a mut [u8],
    rx_buf: &'a mut [u8],
}

impl<'b, T, R: rand_core::RngCore> WebSocketClient<'b, T, R> {
    pub fn new(tcp: T, rng: R, tx_buf: &'b mut [u8], rx_buf: &'b mut [u8]) -> Self {
        Self {
            tcp,
            ws: embedded_websocket::WebSocketClient::new_client(rng),
            tx_buf,
            rx_buf,
        }
    }
}

impl<'b, T: Connect<Remote = str>, R: rand_core::RngCore> WebSocketClient<'b, T, R> {
    pub async fn connect(
        &mut self,
        remote: &str,
        headers: Option<&[&str]>,
    ) -> Result<WebSocketConn<'_, T::Connection<'_>, R>, FramerError<T::Error>> {
        let mut tcp = self.tcp.connect(remote).await.map_err(FramerError::Io)?;
        let mut framer = Framer::new(self.rx_buf, mk_static!(usize, 0), self.tx_buf, &mut self.ws);
        let url = nourl::Url::parse(remote).unwrap();
        let opts = WebSocketOptions {
            path: url.path(),
            host: url.host(),
            origin: remote,
            sub_protocols: None,
            additional_headers: headers,
        };
        framer.connect(&mut tcp, &opts).await.unwrap();
        debug!("websocket connection established");
        Ok(WebSocketConn { conn: tcp, framer })
    }
}

pub struct WebSocketConn<'a, T, R>
where
    R: rand_core::RngCore,
{
    conn: T,
    framer: Framer<'a, R, Client>,
}

impl<'a, T, R> WebSocketConn<'a, T, R>
where
    T: Write + Read,
    R: rand_core::RngCore,
{
    pub async fn send_text(&mut self, text: &str) -> Result<(), FramerError<T::Error>> {
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

impl<'a, T, R> Transport for WebSocketConn<'a, T, R>
where
    T: Read + Write,
    R: rand_core::RngCore,
{
    type Error = FramerError<T::Error>;
    async fn write(&mut self, msg: ProtoMsg<'_>) -> Result<(), Self::Error> {
        use WebSocketSendMessageType::*;
        match msg {
            ProtoMsg::Text(t) => {
                self.framer
                    .write(&mut self.conn, Text, true, t.as_bytes())
                    .await
            }
            ProtoMsg::Binary(items) => self.framer.write(&mut self.conn, Binary, true, items).await,
        }
    }

    async fn read<'b>(&mut self, buf: &'b mut [u8]) -> Result<ProtoMsg<'b>, Self::Error> {
        let ptr = buf.as_mut_ptr() as *mut u8;
        let len = buf.len();
        let buf = || unsafe { core::slice::from_raw_parts_mut(ptr, len) };
        loop {
            let result = self.framer.read(&mut self.conn, buf()).await?;
            match result {
                ReadResult::Binary(b) => break Ok(ProtoMsg::Binary(b)),
                ReadResult::Text(t) => break Ok(ProtoMsg::Text(t)),
                ReadResult::Closed => break Ok(ProtoMsg::Text("")),
                _ => (),
            }
        }
    }
}
