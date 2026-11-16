//! APNG writer: truecolor RGB8, one frame control chunk per merged frame.
//!
//! The animation control chunk comes before the first image, but the frame
//! count is only known at the end. The writer therefore reserves the count,
//! streams every frame, and patches the count and its checksum before the
//! file is verified and moved into place. No frame is held beyond the one that
//! is waiting to learn its delay, and no palette is built (part 12).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::output::OutputSummary;
use crate::writer::{temp_path, verify_and_rename};
use crate::{Error, Frame, OutputFormat, Result};

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A PNG being written frame by frame.
pub struct ApngStream {
    path: PathBuf,
    temp: PathBuf,
    out: Option<BufWriter<File>>,
    width: u32,
    height: u32,
    loop_count: u16,
    pending: Frame,
    has_pending: bool,
    emitted_ms: u32,
    last_ms: u32,
    next_index: u32,
    frames: u32,
    total_ms: u32,
    /// Sequence number shared by fcTL and fdAT chunks.
    sequence: u32,
    /// Where the animation control count sits, for the final patch.
    actl_data_at: u64,
    committed: bool,
}

/// Removes the temp file when a setup step fails before a stream owns it.
struct TempGuard {
    path: PathBuf,
    armed: bool,
}

impl TempGuard {
    fn new(path: PathBuf) -> TempGuard {
        TempGuard { path, armed: true }
    }

    /// Hand the file to the stream that will remove it on its own failure.
    fn keep(mut self) {
        self.armed = false;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Start an APNG. The size is known before the first frame because there is no
/// palette to sample.
pub fn stream_apng(
    path: &Path,
    width: u32,
    height: u32,
    loop_count: u16,
    total_ms: u32,
) -> Result<ApngStream> {
    if width == 0 || height == 0 || width > 65535 || height > 65535 {
        return Err(Error::invalid(format!(
            "APNG size {width}x{height} is out of range"
        )));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_path(path);
    let file = File::create(&temp)?;
    let guard = TempGuard::new(temp.clone());
    let mut out = BufWriter::new(file);

    out.write_all(&SIGNATURE)?;
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit truecolor, no interlace
    write_chunk(&mut out, b"IHDR", &header)?;

    // The count is patched at finish; the play count is final from the start.
    let mut control = Vec::with_capacity(8);
    control.extend_from_slice(&0u32.to_be_bytes());
    control.extend_from_slice(&(loop_count as u32).to_be_bytes());
    let actl_data_at = chunk_data_offset(&mut out)?;
    write_chunk(&mut out, b"acTL", &control)?;

    let pending = Frame::new(width, height)?;
    guard.keep();
    Ok(ApngStream {
        path: path.to_path_buf(),
        temp,
        out: Some(out),
        width,
        height,
        loop_count,
        pending,
        has_pending: false,
        emitted_ms: 0,
        last_ms: 0,
        next_index: 0,
        frames: 0,
        total_ms,
        sequence: 0,
        actl_data_at,
        committed: false,
    })
}

impl ApngStream {
    /// Write the held frame with the delay up to `until_ms`.
    fn emit(&mut self, until_ms: u32) -> Result<()> {
        if !self.has_pending {
            return Ok(());
        }
        let target = crate::time::snap_total(until_ms);
        let delay = target
            .saturating_sub(self.emitted_ms)
            .max(crate::time::MIN_DELAY_MS);
        if delay > 655_350 {
            return Err(Error::invalid(format!(
                "delay {delay} ms is too long for APNG"
            )));
        }
        let emitted_ms = self.emitted_ms.checked_add(delay).ok_or_else(|| {
            Error::invalid(format!(
                "delay {delay} ms would take the total past {} ms",
                u32::MAX
            ))
        })?;
        let idat = idat_payload(&encode_png(self.width, self.height, self.pending.data())?)?;
        let first = self.frames == 0;

        let mut control = Vec::with_capacity(26);
        control.extend_from_slice(&self.sequence.to_be_bytes()); // sequence
        control.extend_from_slice(&self.width.to_be_bytes());
        control.extend_from_slice(&self.height.to_be_bytes());
        control.extend_from_slice(&0u32.to_be_bytes()); // x offset
        control.extend_from_slice(&0u32.to_be_bytes()); // y offset
        control.extend_from_slice(&((delay / 10) as u16).to_be_bytes());
        control.extend_from_slice(&100u16.to_be_bytes()); // hundredths
        control.push(0); // dispose: none
        control.push(0); // blend: source
        self.sequence += 1;

        let out = self
            .out
            .as_mut()
            .ok_or_else(|| Error::io("stream has no file"))?;
        write_chunk(out, b"fcTL", &control)?;
        if first {
            // The first frame is the default image and lives in IDAT.
            write_chunk(out, b"IDAT", &idat)?;
        } else {
            // Later frames need their own sequence number in fdAT.
            let mut framed = Vec::with_capacity(4 + idat.len());
            framed.extend_from_slice(&self.sequence.to_be_bytes());
            framed.extend_from_slice(&idat);
            self.sequence += 1;
            write_chunk(out, b"fdAT", &framed)?;
        }

        self.frames += 1;
        self.emitted_ms = emitted_ms;
        self.has_pending = false;
        Ok(())
    }

    /// Patch the frame count, write the trailer, verify and rename.
    pub fn finish(mut self) -> Result<OutputSummary> {
        if !self.has_pending {
            return Err(Error::invalid("no frames to write"));
        }
        if self.total_ms < self.last_ms {
            return Err(Error::invalid(format!(
                "loop duration {} ms ends before the last frame at {} ms",
                self.total_ms, self.last_ms
            )));
        }
        self.emit(self.total_ms)?;
        let mut out = self
            .out
            .take()
            .ok_or_else(|| Error::io("stream has no file"))?;

        // Patch the animation control count and its checksum.
        let mut control = Vec::with_capacity(8);
        control.extend_from_slice(&self.frames.to_be_bytes());
        control.extend_from_slice(&(self.loop_count as u32).to_be_bytes());
        out.flush()?;
        out.seek(SeekFrom::Start(self.actl_data_at))?;
        out.write_all(&control)?;
        out.seek(SeekFrom::Start(self.actl_data_at + 8))?;
        let control_crc = crc32_parts(&[b"acTL", &control]);
        out.write_all(&control_crc.to_be_bytes())?;
        out.seek(SeekFrom::End(0))?;
        write_chunk(&mut out, b"IEND", &[])?;
        out.flush()?;
        drop(out);

        let summary = OutputSummary {
            frames: self.frames,
            total_ms: self.emitted_ms,
            width: self.width,
            height: self.height,
        };
        verify_and_rename(&self.temp, &self.path, &summary, OutputFormat::Apng)?;
        self.committed = true;
        Ok(summary)
    }
}

/// APNG pushes frames the way the GIF sink does: a repeat extends the delay of
/// the frame already held.
impl ApngStream {
    pub fn push(&mut self, index: u32, pts_us: u64, frame: &Frame) -> Result<()> {
        if index < self.next_index {
            return Err(Error::invalid(format!(
                "frame index {index} went backwards from {}",
                self.next_index
            )));
        }
        if frame.size() != (self.width, self.height) {
            return Err(Error::invalid(format!(
                "frame size {:?} differs from {}x{}",
                frame.size(),
                self.width,
                self.height
            )));
        }
        let ms = pts_ms(pts_us)?;
        if ms < self.last_ms {
            return Err(Error::invalid(format!(
                "presentation time {ms} ms went backwards from {} ms",
                self.last_ms
            )));
        }
        self.last_ms = ms;
        self.next_index = index + 1;

        if self.has_pending && self.pending.data() == frame.data() {
            return Ok(());
        }
        self.emit(ms)?;
        self.pending.data_mut().copy_from_slice(frame.data());
        self.has_pending = true;
        Ok(())
    }
}

impl Drop for ApngStream {
    fn drop(&mut self) {
        if !self.committed {
            self.out.take();
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}

fn pts_ms(pts_us: u64) -> Result<u32> {
    let ms = (pts_us + 500) / 1000;
    u32::try_from(ms).map_err(|_| Error::invalid("presentation time is too large for APNG"))
}

/// The offset of the next chunk's data, which is past the length and type.
fn chunk_data_offset<W: Write + Seek>(out: &mut W) -> Result<u64> {
    out.flush()?;
    let end = out.seek(SeekFrom::End(0))?;
    Ok(end + 8)
}

fn write_chunk<W: Write>(out: &mut W, kind: &[u8; 4], data: &[u8]) -> Result<()> {
    out.write_all(&(data.len() as u32).to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(data)?;
    out.write_all(&crc32_parts(&[kind, data]).to_be_bytes())?;
    Ok(())
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_failed_setup_removes_its_temp_file() {
        let dir = scratch("apng-guard");
        let temp = dir.join("out.tmp.png");
        std::fs::write(&temp, b"half a file").unwrap();
        {
            let _guard = TempGuard::new(temp.clone());
        }
        assert!(!temp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stream_keeps_the_temp_file_it_owns() {
        let dir = scratch("apng-guard-kept");
        let temp = dir.join("out.tmp.png");
        std::fs::write(&temp, b"half a file").unwrap();
        TempGuard::new(temp.clone()).keep();
        assert!(temp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Compress one RGB8 frame into a standalone PNG, through the png crate.
fn encode_png(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| Error::io(format!("cannot start a PNG frame: {err}")))?;
        writer
            .write_image_data(data)
            .map_err(|err| Error::io(format!("cannot encode a PNG frame: {err}")))?;
        writer
            .finish()
            .map_err(|err| Error::io(format!("cannot finish a PNG frame: {err}")))?;
    }
    Ok(bytes)
}

/// The concatenated IDAT payload of a PNG this crate wrote.
fn idat_payload(png: &[u8]) -> Result<Vec<u8>> {
    if png.len() < 8 || png[..8] != SIGNATURE {
        return Err(Error::io("the PNG encoder wrote a bad signature"));
    }
    let mut pos = 8usize;
    let mut payload = Vec::new();
    let mut saw_header = false;
    let mut saw_end = false;
    while pos + 12 <= png.len() {
        let length =
            u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let kind = &png[pos + 4..pos + 8];
        let start = pos + 8;
        let end = start
            .checked_add(length)
            .filter(|end| *end + 4 <= png.len())
            .ok_or_else(|| Error::io("the PNG encoder wrote a truncated chunk"))?;
        match kind {
            b"IHDR" => saw_header = true,
            b"IDAT" => payload.extend_from_slice(&png[start..end]),
            b"IEND" => saw_end = true,
            _ => {}
        }
        pos = end + 4;
        if saw_end {
            break;
        }
    }
    if !saw_header || !saw_end || payload.is_empty() {
        return Err(Error::io("the PNG encoder wrote an incomplete image"));
    }
    Ok(payload)
}

pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    crc32_parts(&[bytes])
}

fn crc32_parts(parts: &[&[u8]]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for part in parts {
        for &byte in *part {
            crc = (crc >> 8) ^ CRC_TABLE[((crc ^ byte as u32) & 0xFF) as usize];
        }
    }
    !crc
}

const CRC_TABLE: [u32; 256] = build_crc_table();

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_known_png_values() {
        // The CRC of "IEND" with no data, as every PNG file carries it.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(crc32(b""), 0);
    }
}
