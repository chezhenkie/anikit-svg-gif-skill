//! GIF89a writer: header, one global palette, per-frame delay, LZW data.
//!
//! The encoder streams. Each frame is quantised by the caller and handed over
//! as indices, so a long animation never exists as a list of frames. Every
//! frame after the first is written as the rectangle that changed, drawn over
//! the canvas the previous frame left (disposal 1, part 03.3).

use std::collections::HashMap;
use std::io::Write;

use crate::output::OutputSummary;
use crate::{Error, Palette, Result};

/// The part of the canvas that differs from the previous frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}

/// A GIF being written to any sink.
pub struct GifEncoder<W: Write> {
    out: W,
    width: u16,
    height: u16,
    bits: u8,
    frames: u32,
    total_ms: u32,
    /// Palette indices of the canvas as the decoder will hold it.
    previous: Vec<u8>,
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
            previous: vec![0; width as usize * height as usize],
        })
    }

    /// Append one frame. `delay_ms` must be a positive multiple of 10, which is
    /// the grid the time crate guarantees.
    pub fn write_frame(&mut self, indices: &[u8], delay_ms: u32) -> Result<()> {
        let width = self.width as usize;
        let height = self.height as usize;
        let want = width * height;
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
        if delay_ms == 0 {
            // A zero delay field is read as 100 ms by most decoders.
            return Err(Error::invalid("delay 0 ms must be at least 10 ms"));
        }
        if delay_ms / 10 > u16::MAX as u32 {
            return Err(Error::invalid(format!(
                "delay {delay_ms} ms is too long for GIF"
            )));
        }

        let rect = if self.frames == 0 {
            Rect {
                left: 0,
                top: 0,
                width,
                height,
            }
        } else {
            // The merge rule upstream absorbs a repeat, so a duplicate that
            // arrives here is written as one pixel and costs nothing.
            changed_rect(&self.previous, indices, width, height).unwrap_or(Rect {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
            })
        };

        // Graphic control extension: disposal 1, no transparency.
        self.out.write_all(&[0x21, 0xF9, 0x04, 0x04])?;
        self.out
            .write_all(&((delay_ms / 10) as u16).to_le_bytes())?;
        self.out.write_all(&[0x00, 0x00])?;

        // Image descriptor: the changed rectangle, no local palette.
        self.out.write_all(&[0x2C])?;
        self.out.write_all(&(rect.left as u16).to_le_bytes())?;
        self.out.write_all(&(rect.top as u16).to_le_bytes())?;
        self.out.write_all(&(rect.width as u16).to_le_bytes())?;
        self.out.write_all(&(rect.height as u16).to_le_bytes())?;
        self.out.write_all(&[0x00])?;

        let min_code_size = self.bits.max(2);
        self.out.write_all(&[min_code_size])?;
        let packed = lzw_encode(
            rect_pixels(indices, width, rect),
            min_code_size,
            rect.width * rect.height / 2 + 16,
        );
        for chunk in packed.chunks(255) {
            self.out.write_all(&[chunk.len() as u8])?;
            self.out.write_all(chunk)?;
        }
        self.out.write_all(&[0x00])?;

        self.remember(indices, rect);
        self.frames += 1;
        self.total_ms += delay_ms;
        Ok(())
    }

    /// Keep the canvas the decoder will hold after this frame.
    fn remember(&mut self, indices: &[u8], rect: Rect) {
        let width = self.width as usize;
        let full = rect.left == 0 && rect.width == width && rect.top == 0;
        if full && rect.height == self.height as usize {
            self.previous.copy_from_slice(indices);
            return;
        }
        for y in 0..rect.height {
            let start = (rect.top + y) * width + rect.left;
            self.previous[start..start + rect.width]
                .copy_from_slice(&indices[start..start + rect.width]);
        }
    }

    /// Write the trailer and report what was written.
    pub fn finish(mut self) -> Result<OutputSummary> {
        self.out.write_all(&[0x3B])?;
        self.out.flush()?;
        Ok(OutputSummary {
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

/// The bounding box of the pixels that differ, or `None` when all agree.
///
/// Rows that match byte for byte are skipped without a per-pixel compare,
/// which keeps a frame with a small change cheap to measure.
fn changed_rect(previous: &[u8], current: &[u8], width: usize, height: usize) -> Option<Rect> {
    let mut left = width;
    let mut right = 0;
    let mut top = height;
    let mut bottom = 0;
    for y in 0..height {
        let start = y * width;
        let before = &previous[start..start + width];
        let after = &current[start..start + width];
        if before == after {
            continue;
        }
        let first = before
            .iter()
            .zip(after)
            .position(|(a, b)| a != b)
            .unwrap_or(0);
        let last = before
            .iter()
            .zip(after)
            .rposition(|(a, b)| a != b)
            .unwrap_or(width - 1);
        left = left.min(first);
        right = right.max(last);
        top = top.min(y);
        bottom = y;
    }
    if top > bottom {
        return None;
    }
    Some(Rect {
        left,
        top,
        width: right - left + 1,
        height: bottom - top + 1,
    })
}

/// The pixels of one rectangle in row order, ready for LZW.
fn rect_pixels(indices: &[u8], width: usize, rect: Rect) -> impl Iterator<Item = u8> + '_ {
    (0..rect.height).flat_map(move |row| {
        let start = (rect.top + row) * width + rect.left;
        indices[start..start + rect.width].iter().copied()
    })
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
fn lzw_encode<I>(indices: I, min_code_size: u8, capacity: usize) -> Vec<u8>
where
    I: IntoIterator<Item = u8>,
{
    let clear = 1u16 << min_code_size;
    let eoi = clear + 1;
    let mut writer = BitWriter::new(capacity);
    let mut dict: HashMap<u32, u16> = HashMap::with_capacity(4096);
    let mut code_size = min_code_size + 1;
    let mut next = eoi + 1;

    writer.push(clear, code_size);
    let mut pixels = indices.into_iter();
    let Some(first) = pixels.next() else {
        writer.push(eoi, code_size);
        return writer.finish();
    };
    let mut prefix = first as u16;
    for pixel in pixels {
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
        let data = lzw_encode([0, 1, 0, 1, 0, 1, 1, 1], 2, 8);
        assert!(!data.is_empty());
    }

    #[test]
    fn the_changed_rectangle_covers_the_difference() {
        // 5 wide, 3 tall, one pixel moved at (1, 1) and (3, 2).
        let before = [0u8; 15];
        let mut after = before;
        after[6] = 1;
        after[13] = 1;
        let rect = changed_rect(&before, &after, 5, 3).unwrap();
        assert_eq!(
            rect,
            Rect {
                left: 1,
                top: 1,
                width: 3,
                height: 2
            }
        );
    }

    #[test]
    fn an_unchanged_frame_has_no_rectangle() {
        let frame = [1u8, 2, 3, 4, 5, 6];
        assert_eq!(changed_rect(&frame, &frame, 3, 2), None);
    }

    #[test]
    fn a_repeated_frame_is_accepted_without_a_rectangle() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut out = Vec::new();
        let mut enc = GifEncoder::new(&mut out, 2, 2, &palette, 0).unwrap();
        enc.write_frame(&[0, 1, 1, 0], 100).unwrap();
        enc.write_frame(&[0, 1, 1, 0], 100).unwrap();
        let summary = enc.finish().unwrap();
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.total_ms, 200);
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
    fn delay_must_not_be_zero() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut out = Vec::new();
        let mut enc = GifEncoder::new(&mut out, 1, 1, &palette, 0).unwrap();
        assert!(enc.write_frame(&[0], 0).is_err());
        assert!(enc.write_frame(&[0], 20).is_ok());
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
