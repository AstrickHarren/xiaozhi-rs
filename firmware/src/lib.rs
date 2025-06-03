#![feature(inherent_str_constructors)]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]
#![feature(type_alias_impl_trait)]
#![no_std]

use core::{fmt::Debug, future::Future};

use bytes::{Bytes, BytesMut};
use embassy_futures::select::select;
use esp_println::{dbg, println};
use log::{debug, info};
use p3::P3Reader;
use proto::{BufTransport, Protocol, Transport};
use serde::{Deserialize, Serialize};

pub mod audio;
pub mod codec;
#[macro_use]
mod r#macro;
pub mod net;
pub mod p3;
pub mod proto;
pub mod util;
pub mod wifi;

#[derive(Debug)]
pub enum RobotState {
    Idle,
    Speaking,
    Listening,
}

pub trait Audio {
    type Error;

    fn play(&mut self, data: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    fn record(&mut self) -> impl Future<Output = Result<BytesMut, Self::Error>>;
}

pub struct DummyAudio;
impl Audio for DummyAudio {
    type Error = ();

    fn play(&mut self, _data: &[u8]) -> impl Future<Output = Result<(), Self::Error>> {
        async { Ok(()) }
    }

    fn record(&mut self) -> impl Future<Output = Result<BytesMut, Self::Error>> {
        async { Ok(BytesMut::new()) }
    }
}

pub struct Robot<P, C> {
    state: RobotState,
    proto: Protocol<P>,
    codec: C,
}

impl<P, C> Robot<P, C>
where
    P: BufTransport,
    C: Audio,
    C::Error: Debug,
    P::Error: Debug,
{
    pub fn new(proto: P, codec: C) -> Self {
        Self {
            state: RobotState::Idle,
            proto: Protocol::new(proto),
            codec,
        }
    }

    // TODO: visable only for debug purpose
    pub async fn set_state(&mut self, state: RobotState) {
        info!("Robot state: {:?}", state);
        self.state = state;
    }

    pub async fn main_loop(mut self) {
        extern crate alloc;
        use alloc::string::ToString;

        self.proto.send_hello().await.unwrap();
        let id = self.proto.recv_hello().await.unwrap().to_string();
        info!("Session Started: {id}");
        dbg!(self.proto.transport.buf_read().await.unwrap());
        self.proto.send_listening(&id).await.unwrap();
        let mut p3 = P3Reader::new(include_bytes!("../assets/wificonfig.p3"));
        while let Some(opus) = p3.next().await.unwrap() {
            debug!("sending {} audio bytes", opus.len());
            self.proto.transport.send_bin(&opus).await.unwrap();
        }
        self.proto.send_listening_stop(&id).await.unwrap();

        loop {
            let msg = self.proto.recv().await.unwrap();
            match msg {
                proto::ServerMsg::Text(t) => {
                    println!("{t:?}");
                }
                proto::ServerMsg::Binary(audio) => self.codec.play(audio).await.unwrap(),
                proto::ServerMsg::Unknown(text) => {
                    println!("Unknown message: {}", text);
                }
            }
        }

        // loop {
        //     match self.state {
        //         RobotState::Idle => self.idle().await.unwrap(),
        //         RobotState::Speaking => self.speaking().await.unwrap(),
        //         RobotState::Listening => self.listening().await.unwrap(),
        //     }
        // }
    }

    // async fn idle(&mut self) -> Result<(), P::Error> {
    //     match self.proto.recv().await? {
    //         Msg::Cmd(Command::Stop) => self.set_state(RobotState::Idle).await,
    //         Msg::Cmd(Command::Speak) => self.set_state(RobotState::Speaking).await,
    //         Msg::Cmd(Command::Listen) => self.set_state(RobotState::Listening).await,
    //         Msg::Audio(_) => (),
    //     };
    //     Ok(())
    // }

    // async fn speaking(&mut self) -> Result<(), P::Error> {
    //     match self.proto.recv().await? {
    //         Msg::Cmd(cmd) => match cmd {
    //             // TODO: reset codec here
    //             Command::Stop => self.set_state(RobotState::Idle).await,
    //             Command::Speak => self.set_state(RobotState::Speaking).await,
    //             Command::Listen => self.set_state(RobotState::Listening).await,
    //         },
    //         Msg::Audio(bin) => self.codec.play(&bin).await.unwrap(),
    //     };

    //     Ok(())
    // }

    // async fn listening(&mut self) -> Result<(), P::Error> {
    //     use embassy_futures::select::Either::*;
    //     match select(self.proto.recv(), self.codec.record()).await {
    //         First(cmd) => match cmd? {
    //             Msg::Cmd(Command::Stop) => self.set_state(RobotState::Idle).await,
    //             Msg::Cmd(Command::Speak) => self.set_state(RobotState::Speaking).await,
    //             Msg::Cmd(Command::Listen) => self.set_state(RobotState::Listening).await,
    //             Msg::Audio(_) => (),
    //         },
    //         Second(bin) => self.proto.send_bin(&bin.unwrap()).await.unwrap(),
    //     }
    //     Ok(())
    // }
}
