//! Reading a file back: frame count, summed delay and size.
//!
//! This is the check that makes a printed path trustworthy. It parses the file
//! we wrote and refuses a mismatch, so a truncated or over-long write is caught
//! before the temp file replaces the target.

use std::path::Path;

use crate::apng::crc32;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputInfo {
    pub frames: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
}

/// Read a GIF back.
pub fn inspect_file(path: &Path) -> Result<OutputInfo> {
    let bytes = std::fs::read(path)?;
    inspect(&bytes)
}

pub fn inspect(bytes: &[u8]) -> Result<OutputInfo> {
    if bytes.len() < 13 {
        return Err(Error::verify("file is too short to be a GIF"));
    }
    if &bytes[0..6] != b"GIF89a" && &bytes[0..6] != b"GIF87a" {
        return Err(Error::verify("missing GIF signature"));
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    let packed = bytes[10];
    let mut pos = 13;
    if packed & 0x80 != 0 {
        let table = 1usize << ((packed & 0x07) + 1);
        pos += table * 3;
    }

    let mut frames = 0u32;
    let mut total_ms = 0u32;
    let mut delay_ms = 0u32;
    while pos < bytes.len() {
        match bytes[pos] {
            0x3B => break,
            0x21 => {
                if pos + 1 >= bytes.len() {
                    return Err(Error::verify("truncated extension block"));
                }
                let label = bytes[pos + 1];
                pos += 2;
                if label == 0xF9 {
                    if pos + 5 > bytes.len() {
                        return Err(Error::verify("truncated graphic control extension"));
                    }
                    let size = bytes[pos] as usize;
                    if size != 4 {
                        return Err(Error::verify("graphic control extension is not 4 bytes"));
                    }
                    delay_ms = u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]) as u32 * 10;
                }
                pos = skip_sub_blocks(bytes, pos)?;
            }
            0x2C => {
                if pos + 10 > bytes.len() {
                    return Err(Error::verify("truncated image descriptor"));
                }
                let local = bytes[pos + 9];
                pos += 10;
                if local & 0x80 != 0 {
                    let table = 1usize << ((local & 0x07) + 1);
                    pos += table * 3;
                }
                if pos >= bytes.len() {
                    return Err(Error::verify("truncated image data"));
                }
                pos += 1; // LZW minimum code size
                pos = skip_sub_blocks(bytes, pos)?;
                frames += 1;
                total_ms += delay_ms;
                delay_ms = 0;
            }
            other => {
                return Err(Error::verify(format!("unexpected block 0x{other:02X}")));
            }
        }
    }

    Ok(OutputInfo {
        frames,
        total_ms,
        width,
        height,
    })
}

fn skip_sub_blocks(bytes: &[u8], mut pos: usize) -> Result<usize> {
    loop {
        let Some(&size) = bytes.get(pos) else {
            return Err(Error::verify("truncated block"));
        };
        pos += 1;
        if size == 0 {
            return Ok(pos);
        }
        pos += size as usize;
        if pos > bytes.len() {
            return Err(Error::verify("truncated block payload"));
        }
    }
}

/// Read an APNG back: the animation control count, the frame control delays,
/// the header size, and every chunk checksum.
pub fn inspect_apng_file(path: &Path) -> Result<OutputInfo> {
    let bytes = std::fs::read(path)?;
    inspect_apng(&bytes)
}

pub fn inspect_apng(bytes: &[u8]) -> Result<OutputInfo> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 8 || bytes[..8] != SIGNATURE {
        return Err(Error::verify("missing PNG signature"));
    }
    let mut pos = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut header_seen = false;
    let mut control: Option<u32> = None;
    let mut frames = 0u32;
    let mut total_ms = 0u32;
    let mut frame_open = false;
    let mut frame_has_data = false;
    let mut end_seen = false;
    let mut sequence = 0u32;

    while pos + 12 <= bytes.len() {
        let length =
            u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                as usize;
        let end = pos
            .checked_add(12)
            .and_then(|at| at.checked_add(length))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::verify("truncated PNG chunk"))?;
        let kind = &bytes[pos + 4..pos + 8];
        let data = &bytes[pos + 8..end - 4];
        let stored = u32::from_be_bytes([
            bytes[end - 4],
            bytes[end - 3],
            bytes[end - 2],
            bytes[end - 1],
        ]);
        if crc32(&bytes[pos + 4..end - 4]) != stored {
            return Err(Error::verify("PNG chunk checksum does not match"));
        }
        match kind {
            b"IHDR" => {
                if data.len() != 13 {
                    return Err(Error::verify("PNG header is not 13 bytes"));
                }
                if data[8] != 8 || data[9] != 2 {
                    return Err(Error::verify("APNG must be 8-bit truecolor"));
                }
                width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                header_seen = true;
            }
            b"acTL" => {
                if data.len() != 8 {
                    return Err(Error::verify("animation control is not 8 bytes"));
                }
                control = Some(u32::from_be_bytes([data[0], data[1], data[2], data[3]]));
            }
            b"fcTL" => {
                if data.len() != 26 {
                    return Err(Error::verify("frame control is not 26 bytes"));
                }
                if frame_open && !frame_has_data {
                    return Err(Error::verify("a frame has no image data"));
                }
                let stored = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                if stored != sequence {
                    return Err(Error::verify(format!(
                        "frame control sequence {stored} should be {sequence}"
                    )));
                }
                sequence += 1;
                let frame_width = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
                let frame_height = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
                if frame_width != width || frame_height != height {
                    return Err(Error::verify("frame size differs from the header"));
                }
                let numerator = u16::from_be_bytes([data[20], data[21]]) as u64;
                let denominator = u16::from_be_bytes([data[22], data[23]]) as u64;
                if denominator == 0 {
                    return Err(Error::verify("frame delay has a zero denominator"));
                }
                total_ms += ((numerator * 1000 + denominator / 2) / denominator) as u32;
                frames += 1;
                frame_open = true;
                frame_has_data = false;
            }
            b"IDAT" => {
                if frame_open {
                    frame_has_data = true;
                }
            }
            b"fdAT" => {
                if data.len() < 4 {
                    return Err(Error::verify("frame data is shorter than its sequence"));
                }
                let stored = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                if stored != sequence {
                    return Err(Error::verify(format!(
                        "frame data sequence {stored} should be {sequence}"
                    )));
                }
                sequence += 1;
                frame_has_data = true;
            }
            b"IEND" => {
                end_seen = true;
                break;
            }
            _ => {}
        }
        pos = end;
    }

    if !header_seen || !end_seen {
        return Err(Error::verify("PNG is missing a header or trailer"));
    }
    if !frame_open || !frame_has_data {
        return Err(Error::verify("APNG has no frames"));
    }
    let Some(control) = control else {
        return Err(Error::verify("APNG has no animation control"));
    };
    if control != frames {
        return Err(Error::verify(format!(
            "animation control names {control} frames, found {frames} frame controls"
        )));
    }

    Ok(OutputInfo {
        frames,
        total_ms,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_non_gif() {
        assert!(inspect(b"not a gif at all").is_err());
        assert!(inspect(&[]).is_err());
    }

    #[test]
    fn rejects_a_non_png() {
        assert!(inspect_apng(b"not a png at all").is_err());
        assert!(inspect_apng(&[]).is_err());
    }
}
