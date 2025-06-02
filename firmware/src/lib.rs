#![feature(inherent_str_constructors)]
#![feature(impl_trait_in_assoc_type)]
#![feature(array_chunks)]
#![feature(type_alias_impl_trait)]
#![no_std]

use core::{fmt::Debug, future::Future};

use bytes::{Bytes, BytesMut};
use embassy_futures::select::select;
use log::info;
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

#[derive(Debug, Serialize, Deserialize)]
pub enum Command {
    Stop,
    Speak,
    Listen,
}

#[derive(Debug)]
pub enum Msg {
    Cmd(Command),
    Audio(Bytes),
}

pub trait Protocol {
    type Error;

    fn recv(&mut self) -> impl Future<Output = Result<Msg, Self::Error>>;
    fn send_bin(&mut self, data: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
}

pub trait Audio {
    type Error;

    fn play(&mut self, data: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    fn record(&mut self) -> impl Future<Output = Result<BytesMut, Self::Error>>;
}

pub struct Robot<P, C> {
    state: RobotState,
    proto: P,
    codec: C,
}

impl<P, C> Robot<P, C>
where
    P: Protocol,
    C: Audio,
    C::Error: Debug,
    P::Error: Debug,
{
    pub fn new(proto: P, codec: C) -> Self {
        Self {
            state: RobotState::Idle,
            proto,
            codec,
        }
    }

    // TODO: visable only for debug purpose
    pub async fn set_state(&mut self, state: RobotState) {
        info!("Robot state: {:?}", state);
        self.state = state;
    }

    pub async fn main_loop(mut self) {
        loop {
            match self.state {
                RobotState::Idle => self.idle().await.unwrap(),
                RobotState::Speaking => self.speaking().await.unwrap(),
                RobotState::Listening => self.listening().await.unwrap(),
            }
        }
    }

    async fn idle(&mut self) -> Result<(), P::Error> {
        match self.proto.recv().await? {
            Msg::Cmd(Command::Stop) => self.set_state(RobotState::Idle).await,
            Msg::Cmd(Command::Speak) => self.set_state(RobotState::Speaking).await,
            Msg::Cmd(Command::Listen) => self.set_state(RobotState::Listening).await,
            Msg::Audio(_) => (),
        };
        Ok(())
    }

    async fn speaking(&mut self) -> Result<(), P::Error> {
        match self.proto.recv().await? {
            Msg::Cmd(cmd) => match cmd {
                // TODO: reset codec here
                Command::Stop => self.set_state(RobotState::Idle).await,
                Command::Speak => self.set_state(RobotState::Speaking).await,
                Command::Listen => self.set_state(RobotState::Listening).await,
            },
            Msg::Audio(bin) => self.codec.play(&bin).await.unwrap(),
        };

        Ok(())
    }

    async fn listening(&mut self) -> Result<(), P::Error> {
        use embassy_futures::select::Either::*;
        match select(self.proto.recv(), self.codec.record()).await {
            First(cmd) => match cmd? {
                Msg::Cmd(Command::Stop) => self.set_state(RobotState::Idle).await,
                Msg::Cmd(Command::Speak) => self.set_state(RobotState::Speaking).await,
                Msg::Cmd(Command::Listen) => self.set_state(RobotState::Listening).await,
                Msg::Audio(_) => (),
            },
            Second(bin) => self.proto.send_bin(&bin.unwrap()).await.unwrap(),
        }
        Ok(())
    }
}
