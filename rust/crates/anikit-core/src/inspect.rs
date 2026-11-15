//! Reading a GIF back: frame count, summed delay and size.
//!
//! This is the check that makes a printed path trustworthy. It parses the file
//! we wrote and refuses a mismatch, so a truncated or over-long write is caught
//! before the temp file replaces the target.

use std::path::Path;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GifInfo {
    pub frames: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
}

pub fn inspect_file(path: &Path) -> Result<GifInfo> {
    let bytes = std::fs::read(path)?;
    inspect(&bytes)
}

pub fn inspect(bytes: &[u8]) -> Result<GifInfo> {
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

    Ok(GifInfo {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_non_gif() {
        assert!(inspect(b"not a gif at all").is_err());
        assert!(inspect(&[]).is_err());
    }
}
