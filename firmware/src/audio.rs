use esp_hal::{
    dma::{DmaChannelFor, DmaPriority},
    dma_circular_buffers,
    gpio::interconnect::{PeripheralInput, PeripheralOutput},
    i2s::master::{AnyI2s, I2s, I2sRx, I2sTx, RegisterAccess},
    peripheral::Peripheral,
    time::Rate,
    Async,
};

pub struct I2sConfig<WS, BCLK, I2S, DMA> {
    pub i2s: I2S,
    pub dma: DMA,
    pub ws: WS,
    pub bclk: BCLK,
}

impl<WS, BCLK, I2S, DMA> I2sConfig<WS, BCLK, I2S, DMA>
where
    I2S: Peripheral<P: RegisterAccess> + 'static,
    DMA: Peripheral<P: DmaChannelFor<AnyI2s>> + 'static,
    WS: Peripheral<P: PeripheralOutput> + 'static,
    BCLK: Peripheral<P: PeripheralOutput> + 'static,
{
    pub fn build_i2s(
        i2s: impl Peripheral<P: RegisterAccess> + 'static,
        dma: impl Peripheral<P: DmaChannelFor<AnyI2s>> + 'static,
    ) -> (I2s<'static, Async>, &'static mut [u8], &'static mut [u8]) {
        let (rx_buf, rx_d, tx_buf, tx_d) = dma_circular_buffers!(4092 * 10, 0);
        let i2s = I2s::new(
            i2s,
            esp_hal::i2s::master::Standard::Philips,
            esp_hal::i2s::master::DataFormat::Data32Channel32,
            Rate::from_khz(16),
            dma,
            rx_d,
            tx_d,
        )
        .into_async();
        (i2s, rx_buf, tx_buf)
    }

    pub fn build_input(
        self,
        din: impl Peripheral<P: PeripheralInput> + 'static,
    ) -> (&'static mut [u8], I2sRx<'static, Async>) {
        {
            let (i2s, rx_buf, _) = Self::build_i2s(self.i2s, self.dma);
            let i2s_rx = {
                let mut i2s = i2s
                    .i2s_rx
                    .with_ws(self.ws)
                    .with_bclk(self.bclk)
                    .with_din(din);
                i2s.rx_channel.set_priority(DmaPriority::Priority0);
                i2s.build()
            };
            (rx_buf, i2s_rx)
        }
    }

    pub fn build_output(
        self,
        dout: impl Peripheral<P: PeripheralOutput> + 'static,
    ) -> (&'static mut [u8], I2sTx<'static, Async>) {
        {
            let (i2s, _, tx_buf) = Self::build_i2s(self.i2s, self.dma);
            let i2s_tx = {
                let mut i2s = i2s
                    .i2s_tx
                    .with_ws(self.ws)
                    .with_bclk(self.bclk)
                    .with_dout(dout);
                i2s.tx_channel.set_priority(DmaPriority::Priority0);
                i2s.build()
            };
            (tx_buf, i2s_tx)
        }
    }
}
