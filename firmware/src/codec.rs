use bytes::{Buf, BufMut, BytesMut};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::NoopRawMutex,
    channel::{Receiver, Sender},
};
use esp_hal::{
    dma::DmaError,
    i2s::master::{I2sRx, I2sTx},
    Async,
};
use esp_println::dbg;
use log::{error, info, trace, warn};
use opus::{Decoder, Encoder};

use crate::{mk_ch, util::BytesMutExtend, Audio};

pub struct I2sSimplex {
    mic_rx: Receiver<'static, NoopRawMutex, BytesMut, 10>,
    speaker_tx: Sender<'static, NoopRawMutex, BytesMut, 10>,
}

pub struct I2sSimplexConfig {
    pub mic_rx: I2sRx<'static, Async>,
    pub mic_buf: &'static mut [u8],
    pub speaker_tx: I2sTx<'static, Async>,
    pub speaker_buf: &'static mut [u8],
}

impl I2sSimplex {
    pub fn new(s: &Spawner, config: I2sSimplexConfig) -> Self {
        let (speaker_tx, speaker_rx) = mk_ch!(10);
        let (mic_tx, mic_rx) = mk_ch!(10);
        s.spawn(listen_task(mic_tx, config.mic_rx, config.mic_buf))
            .unwrap();
        s.spawn(speak_task(
            speaker_rx,
            config.speaker_tx,
            config.speaker_buf,
        ))
        .unwrap();

        Self { mic_rx, speaker_tx }
    }
}

impl Audio for I2sSimplex {
    type Error = ();

    async fn play(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        self.speaker_tx.send(data.into()).await;
        Ok(())
    }

    async fn record(&mut self) -> Result<BytesMut, Self::Error> {
        Ok(self.mic_rx.receive().await)
    }
}

#[embassy_executor::task]
async fn listen_task(
    sender: Sender<'static, NoopRawMutex, BytesMut, 10>,
    i2s_rx: I2sRx<'static, Async>,
    rx_buf: &'static mut [u8],
) {
    info!("start continuous i2s mic");
    const FRAME_SIZE: usize = 960;
    let mut data = [0; 1024 * 5];
    let mut remain = BytesMut::new();

    let mut enc = Encoder::new(16000, opus::Channels::Mono, opus::Application::Audio).unwrap();
    enc.set_complexity(3).unwrap();
    let mut transfer = i2s_rx.read_dma_circular_async(rx_buf).unwrap();
    loop {
        use esp_hal::i2s::master::Error;
        match transfer.pop(&mut data).await {
            Ok(n) => {
                // INMP441 puts data into 24bits
                let data = data[..n]
                    .array_chunks::<4>()
                    .step_by(2) // skip right channel
                    .map(|c| i32::from_le_bytes(*c) >> 12) // shift right by 12 bits (not sure why)
                    .map(|c| c.clamp(i16::MIN as _, i16::MAX as _) as i16);
                remain.extend(data.flat_map(|b| b.to_le_bytes()));

                while remain.len() >= FRAME_SIZE * 2 {
                    let frame = remain.split_to(FRAME_SIZE * 2);
                    let mut out = BytesMut::with_capacity(200);
                    let n = enc.encode(frame.transmute(), out.transmute_cap()).unwrap();
                    unsafe { out.advance_mut(n) };
                    sender.try_send(out[..n].into()).ok(); // Ignore buffer full
                }
            }
            Err(Error::DmaError(DmaError::Late)) => warn!("Dma late for mic"),
            Err(Error::DmaError(DmaError::BufferTooSmall)) => error!("Buffer too small for mic"),
            Err(e) => panic!("Unexpected error: {e:?}"),
        }
    }
}

#[embassy_executor::task]
async fn speak_task(
    receiver: Receiver<'static, NoopRawMutex, BytesMut, 10>,
    i2s_tx: I2sTx<'static, Async>,
    tx_buf: &'static mut [u8],
) {
    info!("start continuous i2s speaker");
    let mut transfer = i2s_tx.write_dma_circular_async(tx_buf).unwrap();

    let pcm = &mut [0; 960];
    // FIXME: need to reset decoder every time a new udp stream is received
    let mut dec = Decoder::new(16000, opus::Channels::Mono).unwrap();
    loop {
        trace!("SPEAK: queued {} audio samples", receiver.len());

        match receiver.try_receive() {
            Ok(data) => dec.decode(&data, pcm, false).unwrap(),
            Err(_) => dec.decode(&[], pcm, false).unwrap(),
        };

        let volume_factor: i32 = 32112; // WARNING: 70% volume
        let mut data: BytesMut = pcm
            .into_iter()
            .map(|p| {
                let temp = *p as i64 * volume_factor as i64;
                // clamp to i32 range
                let clamped = temp.max(i32::MIN as i64).min(i32::MAX as i64) as i32;
                clamped as i32
            })
            .flat_map(|x| [0, x])
            .flat_map(|x| x.to_le_bytes())
            .collect();

        // Push all bytes (audio or silence) into I²S
        while !data.is_empty() {
            let n = transfer.push(&data).await.unwrap();
            data.advance(n);
        }
    }
}
