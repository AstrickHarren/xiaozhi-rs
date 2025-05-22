use core::convert::Infallible;

use bytes::{Buf, Bytes, BytesMut};
use embedded_io::{BufRead, Read, ReadExactError};
use esp_println::println;

pub struct P3Reader<'a> {
    data: &'a [u8],
}

impl<'a> P3Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    pub fn next(&mut self) -> Result<Option<BytesMut>, ReadExactError<Infallible>> {
        if self.data.is_empty() {
            return Ok(None);
        }

        let mut header = [0; 4];
        self.data.read_exact(&mut header)?;
        let size = u16::from_be_bytes([header[2], header[3]]);

        let mut buf = BytesMut::zeroed(size as usize);
        self.data.read_exact(&mut buf)?;
        Ok(Some(buf))
    }
}

impl Iterator for P3Reader<'_> {
    type Item = Result<BytesMut, ReadExactError<Infallible>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next().transpose()
    }
}
