//! GIF89a writer: header, one global palette, per-frame delay, LZW data.
//!
//! The encoder streams. Each frame is quantised by the caller and handed over
//! as indices, so a long animation never exists as a list of frames.

use std::collections::HashMap;
use std::io::Write;

use crate::{Error, Palette, Result};

/// What the encoder produced, for the caller's summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GifSummary {
    pub frames: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
}

/// A GIF being written to any sink.
pub struct GifEncoder<W: Write> {
    out: W,
    width: u16,
    height: u16,
    bits: u8,
    frames: u32,
    total_ms: u32,
}

impl<W: Write> GifEncoder<W> {
    /// Write the header, the global palette and the loop block.
    pub fn new(
        mut out: W,
        width: u32,
        height: u32,
        palette: &Palette,
        loop_count: u16,
    ) -> Result<GifEncoder<W>> {
        if width == 0 || height == 0 || width > 65535 || height > 65535 {
            return Err(Error::invalid(format!(
                "GIF size {width}x{height} is out of range"
            )));
        }
        if palette.is_empty() || palette.len() > 255 {
            return Err(Error::invalid(
                "palette must hold between 1 and 255 colours",
            ));
        }
        let bits = bits_for(palette.len());
        let table = 1usize << bits;

        out.write_all(b"GIF89a")?;
        out.write_all(&(width as u16).to_le_bytes())?;
        out.write_all(&(height as u16).to_le_bytes())?;
        out.write_all(&[0x80 | ((bits - 1) << 4) | (bits - 1), 0, 0])?;
        for i in 0..table {
            let colour = palette.colors().get(i).copied().unwrap_or([0, 0, 0]);
            out.write_all(&colour)?;
        }

        // NETSCAPE2.0 application extension, loop count 0 means forever.
        out.write_all(&[0x21, 0xFF, 0x0B])?;
        out.write_all(b"NETSCAPE2.0")?;
        out.write_all(&[0x03, 0x01])?;
        out.write_all(&loop_count.to_le_bytes())?;
        out.write_all(&[0x00])?;

        Ok(GifEncoder {
            out,
            width: width as u16,
            height: height as u16,
            bits,
            frames: 0,
            total_ms: 0,
        })
    }

    /// Append one frame. `delay_ms` must be a multiple of 10, which is the grid
    /// the time crate guarantees.
    pub fn write_frame(&mut self, indices: &[u8], delay_ms: u32) -> Result<()> {
        let want = self.width as usize * self.height as usize;
        if indices.len() != want {
            return Err(Error::invalid(format!(
                "frame has {} indices, expected {want}",
                indices.len()
            )));
        }
        if !delay_ms.is_multiple_of(10) {
            return Err(Error::invalid(format!(
                "delay {delay_ms} ms is not a multiple of 10"
            )));
        }
        if delay_ms / 10 > u16::MAX as u32 {
            return Err(Error::invalid(format!(
                "delay {delay_ms} ms is too long for GIF"
            )));
        }

        // Graphic control extension: disposal 1, no transparency.
        self.out.write_all(&[0x21, 0xF9, 0x04, 0x04])?;
        self.out
            .write_all(&((delay_ms / 10) as u16).to_le_bytes())?;
        self.out.write_all(&[0x00, 0x00])?;

        // Image descriptor: full frame, no local palette, not interlaced.
        self.out.write_all(&[0x2C, 0x00, 0x00, 0x00, 0x00])?;
        self.out.write_all(&self.width.to_le_bytes())?;
        self.out.write_all(&self.height.to_le_bytes())?;
        self.out.write_all(&[0x00])?;

        let min_code_size = self.bits.max(2);
        self.out.write_all(&[min_code_size])?;
        let packed = lzw_encode(indices, min_code_size);
        for chunk in packed.chunks(255) {
            self.out.write_all(&[chunk.len() as u8])?;
            self.out.write_all(chunk)?;
        }
        self.out.write_all(&[0x00])?;

        self.frames += 1;
        self.total_ms += delay_ms;
        Ok(())
    }

    /// Write the trailer and report what was written.
    pub fn finish(mut self) -> Result<GifSummary> {
        self.out.write_all(&[0x3B])?;
        self.out.flush()?;
        Ok(GifSummary {
            frames: self.frames,
            total_ms: self.total_ms,
            width: self.width as u32,
            height: self.height as u32,
        })
    }
}

fn bits_for(colours: usize) -> u8 {
    let mut bits = 2u8;
    while (1usize << bits) < colours && bits < 8 {
        bits += 1;
    }
    bits
}

struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    bits: u8,
}

impl BitWriter {
    fn new(capacity: usize) -> BitWriter {
        BitWriter {
            out: Vec::with_capacity(capacity),
            acc: 0,
            bits: 0,
        }
    }

    fn push(&mut self, code: u16, size: u8) {
        self.acc |= (code as u32) << self.bits;
        self.bits += size;
        while self.bits >= 8 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.out.push((self.acc & 0xFF) as u8);
        }
        self.out
    }
}

/// GIF variable-length LZW. Codes are packed least significant bit first.
fn lzw_encode(indices: &[u8], min_code_size: u8) -> Vec<u8> {
    let clear = 1u16 << min_code_size;
    let eoi = clear + 1;
    let mut writer = BitWriter::new(indices.len() / 2 + 16);
    let mut dict: HashMap<u32, u16> = HashMap::with_capacity(4096);
    let mut code_size = min_code_size + 1;
    let mut next = eoi + 1;

    writer.push(clear, code_size);
    let mut prefix = indices[0] as u16;
    for &pixel in &indices[1..] {
        let key = ((prefix as u32) << 8) | pixel as u32;
        if let Some(&code) = dict.get(&key) {
            prefix = code;
            continue;
        }
        writer.push(prefix, code_size);
        dict.insert(key, next);
        next += 1;
        // The decoder's table is one entry behind, so the width grows when the
        // encoder's next code would no longer fit even for the decoder.
        if next > (1 << code_size) && code_size < 12 {
            code_size += 1;
        }
        if next == 4096 {
            writer.push(clear, code_size);
            dict.clear();
            next = eoi + 1;
            code_size = min_code_size + 1;
        }
        prefix = pixel as u16;
    }
    writer.push(prefix, code_size);
    // Reading this last code makes the decoder add one more entry, which may
    // widen its code size before it reads the end code.
    if next >= (1 << code_size) && code_size < 12 {
        code_size += 1;
    }
    writer.push(eoi, code_size);
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lzw_output_is_not_empty() {
        let data = lzw_encode(&[0, 1, 0, 1, 0, 1, 1, 1], 2);
        assert!(!data.is_empty());
    }

    #[test]
    fn delay_must_be_on_the_grid() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut out = Vec::new();
        let mut enc = GifEncoder::new(&mut out, 1, 1, &palette, 0).unwrap();
        assert!(enc.write_frame(&[0], 105).is_err());
        assert!(enc.write_frame(&[0], 100).is_ok());
    }

    #[test]
    fn frame_length_is_checked() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut out = Vec::new();
        let mut enc = GifEncoder::new(&mut out, 2, 2, &palette, 0).unwrap();
        assert!(enc.write_frame(&[0, 1, 0], 100).is_err());
        assert!(enc.write_frame(&[0, 1, 0, 1], 100).is_ok());
    }
}
