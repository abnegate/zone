use std::io::{Result, Write};

use super::capture::Capture;

pub(super) struct Writer;

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> Result<usize> {
        Capture::append(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}
