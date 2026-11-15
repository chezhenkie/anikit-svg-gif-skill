//! A minimal PNG writer for `--preview`.
//!
//! The preview is one still frame, so the IDAT stream uses stored deflate
//! blocks: valid zlib without pulling a compressor into the binary.

use std::fs;
use std::path::Path;

use anikit_core::{Error, Frame, Result};

/// Write one frame as an 8-bit RGB PNG.
pub fn write_png(path: &Path, frame: &Frame) -> Result<()> {
    let stride = frame.width() as usize * 3;
    let mut raw = Vec::with_capacity((stride + 1) * frame.height() as usize);
    for row in 0..frame.height() as usize {
        raw.push(0);
        raw.extend_from_slice(&frame.data()[row * stride..(row + 1) * stride]);
    }

    let mut png = Vec::with_capacity(raw.len() + 128);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&frame.width().to_be_bytes());
    header.extend_from_slice(&frame.height().to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    chunk(&mut png, b"IEND", &[]);

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| Error::io(format!("cannot create {}: {err}", parent.display())))?;
    }
    fs::write(path, &png)
        .map_err(|err| Error::io(format!("cannot write {}: {err}", path.display())))
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 65535 * 5 + 16);
    out.extend_from_slice(&[0x78, 0x01]);
    let mut offset = 0usize;
    loop {
        let take = (data.len() - offset).min(65535);
        let last = offset + take == data.len();
        out.push(u8::from(last));
        let len = take as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(&data[offset..offset + take]);
        offset += take;
        if last {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        let index = ((crc ^ *byte as u32) & 0xFF) as usize;
        crc = CRC_TABLE[index] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

const fn make_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0usize;
    while n < 256 {
        let mut value = n as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xEDB8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[n] = value;
        n += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = make_crc_table();

#[cfg(test)]
mod tests {
    use super::*;

    fn read_be(data: &[u8], at: usize) -> u32 {
        u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    #[test]
    fn a_preview_round_trips_its_pixels() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/png")
            .join("preview");
        let _ = fs::remove_dir_all(&dir);
        let mut frame = Frame::new(3, 2).unwrap();
        frame.set_pixel(0, 0, [255, 0, 0]);
        frame.set_pixel(2, 1, [0, 0, 255]);
        let path = dir.join("still.png");
        write_png(&path, &frame).unwrap();

        let png = fs::read(&path).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(read_be(&png, 8), 13);
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(read_be(&png, 16), 3);
        assert_eq!(read_be(&png, 20), 2);
        assert_eq!(&png[24..26], &[8, 2]);

        let idat_len = read_be(&png, 33) as usize;
        let idat_at = 41;
        assert_eq!(&png[37..41], b"IDAT");
        assert!(idat_len > 2);
        let zlib = &png[idat_at..idat_at + idat_len];
        assert_eq!(&zlib[..2], &[0x78, 0x01]);
        assert_eq!(zlib[2], 1);
        let stored = &zlib[7..zlib.len() - 4];
        assert_eq!(stored[0], 0);
        assert_eq!(&stored[1..4], &[255, 0, 0]);
        assert_eq!(stored[10], 0);
        assert_eq!(&stored[17..20], &[0, 0, 255]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checksums_match_known_values() {
        assert_eq!(adler32(b"abc"), 0x024d_0127);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
