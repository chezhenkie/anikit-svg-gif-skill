//! Writing frames as they arrive, against a presentation clock.
//!
//! The format is chosen from the output path before any frame is rendered, so
//! an APNG never builds a palette (part 12). The GIF path needs the palette to
//! exist before its first frame: the caller renders up to twelve sample frames
//! first. Either writer takes each frame with its presentation time, derives
//! the delay, merges a run of identical frames, and holds the last frame back
//! until the loop duration is known. Only one frame is ever buffered, which is
//! the two-slot pipeline of part 03.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::apng::{ApngStream, stream_apng};
use crate::encode::GifEncoder;
use crate::output::{OutputFormat, OutputSummary};
use crate::palette::Palette;
use crate::quantize::{Dither, Quantizer};
use crate::time::{MIN_DELAY_MS, snap_total};
use crate::writer::{temp_path, verify_and_rename};
use crate::{Error, Frame, Result};

/// Everything the stream needs that is not a frame or a time.
#[derive(Debug, Clone)]
pub struct StreamOptions {
    pub format: OutputFormat,
    pub width: u32,
    pub height: u32,
    pub loop_count: u16,
    pub colors: usize,
    pub dither: Dither,
    /// The length of the loop, which fixes the last frame's delay.
    pub total_ms: u32,
}

/// The contract a renderer pushes frames into (part 03, section 3.2).
pub trait FrameSink {
    /// Push one frame at its presentation time in microseconds.
    ///
    /// `index` is the frame's position in the plan; it must not go backwards.
    /// A frame identical to the one held for the previous push extends that
    /// frame's delay instead of producing another one.
    fn push(&mut self, index: u32, pts_us: u64, frame: &Frame) -> Result<()>;

    /// Write the final frame, verify the file and move it into place.
    fn finish(self) -> Result<OutputSummary>;
}

/// A GIF being written frame by frame.
pub struct GifStream {
    path: PathBuf,
    temp: PathBuf,
    width: u32,
    height: u32,
    quantizer: Quantizer,
    encoder: Option<GifEncoder<BufWriter<File>>>,
    options: StreamOptions,
    pending: Option<Vec<u8>>,
    emitted_ms: u32,
    last_ms: u32,
    next_index: u32,
    committed: bool,
}

/// Either frame-based writer, chosen by the output path.
pub enum OutputStream {
    Gif(GifStream),
    Apng(ApngStream),
}

impl FrameSink for OutputStream {
    fn push(&mut self, index: u32, pts_us: u64, frame: &Frame) -> Result<()> {
        match self {
            OutputStream::Gif(stream) => stream.push(index, pts_us, frame),
            OutputStream::Apng(stream) => stream.push(index, pts_us, frame),
        }
    }

    fn finish(self) -> Result<OutputSummary> {
        match self {
            OutputStream::Gif(stream) => stream.finish(),
            OutputStream::Apng(stream) => stream.finish(),
        }
    }
}

/// Open the writer the output path asks for.
///
/// GIF takes its palette from `samples`, which must not be empty. APNG needs
/// no palette, so `samples` may be empty and the size comes from the options.
pub fn stream_output(
    path: &Path,
    samples: &[Frame],
    options: StreamOptions,
) -> Result<OutputStream> {
    match options.format {
        OutputFormat::Gif => Ok(OutputStream::Gif(gif_stream(path, samples, options)?)),
        OutputFormat::Apng => Ok(OutputStream::Apng(stream_apng(
            path,
            options.width,
            options.height,
            options.loop_count,
            options.total_ms,
        )?)),
        OutputFormat::Svg => Err(Error::invalid(
            "SVG output is only written by the tween route",
        )),
    }
}

/// Start a GIF stream from the frames that build the palette.
fn gif_stream(path: &Path, samples: &[Frame], options: StreamOptions) -> Result<GifStream> {
    let Some(first) = samples.first() else {
        return Err(Error::invalid("no frames to sample a palette from"));
    };
    if first.size() != (options.width, options.height) {
        return Err(Error::invalid(format!(
            "sample size {:?} differs from {}x{}",
            first.size(),
            options.width,
            options.height
        )));
    }
    for frame in samples {
        if frame.size() != (options.width, options.height) {
            return Err(Error::invalid(format!(
                "sample size {:?} differs from {}x{}",
                frame.size(),
                options.width,
                options.height
            )));
        }
    }

    let palette = Palette::from_frames(samples, options.colors)?;
    let quantizer = Quantizer::new(palette.clone());
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_path(path);
    let file = File::create(&temp)?;
    let encoder = match GifEncoder::new(
        BufWriter::new(file),
        options.width,
        options.height,
        &palette,
        options.loop_count,
    ) {
        Ok(encoder) => encoder,
        Err(err) => {
            let _ = std::fs::remove_file(&temp);
            return Err(err);
        }
    };

    Ok(GifStream {
        path: path.to_path_buf(),
        temp,
        width: options.width,
        height: options.height,
        quantizer,
        encoder: Some(encoder),
        options,
        pending: None,
        emitted_ms: 0,
        last_ms: 0,
        next_index: 0,
        committed: false,
    })
}

impl GifStream {
    /// Write the held frame and sum its delay up to `until_ms`.
    fn emit(&mut self, until_ms: u32) -> Result<()> {
        let Some(indices) = self.pending.take() else {
            return Ok(());
        };
        let target = snap_total(until_ms);
        let delay = target.saturating_sub(self.emitted_ms).max(MIN_DELAY_MS);
        let emitted_ms = self.emitted_ms.checked_add(delay).ok_or_else(|| {
            Error::invalid(format!(
                "delay {delay} ms would take the total past {} ms",
                u32::MAX
            ))
        })?;
        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| Error::io("stream has no encoder"))?;
        encoder.write_frame(&indices, delay)?;
        self.emitted_ms = emitted_ms;
        Ok(())
    }
}

impl FrameSink for GifStream {
    fn push(&mut self, index: u32, pts_us: u64, frame: &Frame) -> Result<()> {
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

        let indices = self.quantizer.quantize(frame, self.options.dither);
        if let Some(pending) = self.pending.as_ref()
            && pending == &indices
        {
            return Ok(());
        }
        self.emit(ms)?;
        self.pending = Some(indices);
        Ok(())
    }

    fn finish(mut self) -> Result<OutputSummary> {
        if self.pending.is_none() {
            return Err(Error::invalid("no frames to write"));
        }
        if self.options.total_ms < self.last_ms {
            return Err(Error::invalid(format!(
                "loop duration {} ms ends before the last frame at {} ms",
                self.options.total_ms, self.last_ms
            )));
        }
        self.emit(self.options.total_ms)?;
        let encoder = self
            .encoder
            .take()
            .ok_or_else(|| Error::io("stream has no encoder"))?;
        let summary = encoder.finish()?;
        verify_and_rename(&self.temp, &self.path, &summary, OutputFormat::Gif)?;
        self.committed = true;
        Ok(summary)
    }
}

impl Drop for GifStream {
    fn drop(&mut self) {
        if !self.committed {
            // Closing the encoder first releases the file handle, which a
            // remove needs on Windows.
            self.encoder.take();
            let _ = std::fs::remove_file(&self.temp);
        }
    }
}

fn pts_ms(pts_us: u64) -> Result<u32> {
    let ms = (pts_us + 500) / 1000;
    u32::try_from(ms).map_err(|_| Error::invalid("presentation time is too large for GIF"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect_file;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn solid(rgb: [u8; 3]) -> Frame {
        let mut frame = Frame::new(8, 8).unwrap();
        frame.fill(rgb);
        frame
    }

    fn delays(path: &Path) -> Vec<u32> {
        let bytes = std::fs::read(path).unwrap();
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let mut out = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            out.push(frame.delay as u32 * 10);
        }
        out
    }

    fn options(total_ms: u32) -> StreamOptions {
        StreamOptions {
            format: OutputFormat::Gif,
            width: 8,
            height: 8,
            loop_count: 0,
            colors: 255,
            dither: Dither::None,
            total_ms,
        }
    }

    fn apng_options(total_ms: u32) -> StreamOptions {
        StreamOptions {
            format: OutputFormat::Apng,
            ..options(total_ms)
        }
    }

    #[test]
    fn frames_are_delayed_by_their_presentation_times() {
        let dir = scratch("stream-delays");
        let path = dir.join("clock.gif");
        let frames = [solid([255, 0, 0]), solid([0, 255, 0]), solid([0, 0, 255])];
        let mut stream = stream_output(&path, &frames, options(100)).unwrap();
        stream.push(0, 0, &frames[0]).unwrap();
        stream.push(1, 33_000, &frames[1]).unwrap();
        stream.push(2, 66_000, &frames[2]).unwrap();
        let summary = stream.finish().unwrap();
        assert_eq!(summary.frames, 3);
        assert_eq!(summary.total_ms, 100);
        assert_eq!(delays(&path), vec![30, 40, 30]);
        assert!(!dir.join("clock.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_held_frame_merges_into_one_run() {
        let dir = scratch("stream-hold");
        let path = dir.join("hold.gif");
        let red = solid([255, 0, 0]);
        let blue = solid([0, 0, 255]);
        let samples = [red.clone(), blue.clone()];
        let mut stream = stream_output(&path, &samples, options(300)).unwrap();
        stream.push(0, 0, &red).unwrap();
        stream.push(1, 100_000, &red).unwrap();
        stream.push(2, 200_000, &blue).unwrap();
        let summary = stream.finish().unwrap();
        assert_eq!(summary.frames, 2);
        assert_eq!(delays(&path), vec![200, 100]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_last_delay_comes_from_the_loop_duration() {
        let dir = scratch("stream-last");
        let path = dir.join("last.gif");
        let frame = solid([9, 9, 9]);
        let mut stream = stream_output(&path, std::slice::from_ref(&frame), options(450)).unwrap();
        stream.push(0, 0, &frame).unwrap();
        let summary = stream.finish().unwrap();
        assert_eq!(summary.total_ms, 450);
        assert_eq!(delays(&path), vec![450]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stream_and_a_materialised_run_are_byte_identical() {
        let dir = scratch("stream-determinism");
        let frames = [solid([10, 20, 30]), solid([40, 50, 60])];
        let mut first = stream_output(&dir.join("a.gif"), &frames, options(200)).unwrap();
        let mut second = stream_output(&dir.join("b.gif"), &frames, options(200)).unwrap();
        for (index, frame) in frames.iter().enumerate() {
            let pts = index as u64 * 100_000;
            first.push(index as u32, pts, frame).unwrap();
            second.push(index as u32, pts, frame).unwrap();
        }
        first.finish().unwrap();
        second.finish().unwrap();
        assert_eq!(
            std::fs::read(dir.join("a.gif")).unwrap(),
            std::fs::read(dir.join("b.gif")).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_stream_drops_its_temp_file() {
        let dir = scratch("stream-failure");
        let path = dir.join("bad.gif");
        let frame = solid([1, 2, 3]);
        let mut stream = stream_output(&path, std::slice::from_ref(&frame), options(100)).unwrap();
        stream.push(0, 0, &frame).unwrap();
        let wrong = Frame::new(4, 4).unwrap();
        assert!(stream.push(1, 100_000, &wrong).is_err());
        drop(stream);
        assert!(!dir.join("bad.tmp.gif").exists());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_stream_writes_nothing() {
        let dir = scratch("stream-empty");
        let path = dir.join("empty.gif");
        let frame = solid([1, 2, 3]);
        let stream = stream_output(&path, std::slice::from_ref(&frame), options(100)).unwrap();
        assert!(stream.finish().is_err());
        assert!(!path.exists());
        assert!(!dir.join("empty.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_loop_cannot_end_before_the_last_frame() {
        let dir = scratch("stream-short");
        let path = dir.join("short.gif");
        let frame = solid([1, 2, 3]);
        let mut stream = stream_output(&path, std::slice::from_ref(&frame), options(50)).unwrap();
        stream.push(0, 100_000, &frame).unwrap();
        assert!(stream.finish().is_err());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sample_sizes_must_agree() {
        let dir = scratch("stream-samples");
        let path = dir.join("samples.gif");
        let a = solid([1, 2, 3]);
        let b = Frame::new(4, 4).unwrap();
        assert!(stream_output(&path, &[a, b], options(100)).is_err());
        assert!(!dir.join("samples.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_written_file_is_verifiable() {
        let dir = scratch("stream-verify");
        let path = dir.join("ok.gif");
        let frame = solid([5, 6, 7]);
        let mut stream = stream_output(&path, std::slice::from_ref(&frame), options(100)).unwrap();
        stream.push(0, 0, &frame).unwrap();
        stream.finish().unwrap();
        let info = inspect_file(&path).unwrap();
        assert_eq!(info.frames, 1);
        assert_eq!(info.total_ms, 100);
        assert_eq!((info.width, info.height), (8, 8));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_apng_stream_needs_no_samples() {
        let dir = scratch("stream-apng");
        let path = dir.join("out.png");
        let red = solid([255, 0, 0]);
        let blue = solid([0, 0, 255]);
        let mut stream = stream_output(&path, &[], apng_options(300)).unwrap();
        stream.push(0, 0, &red).unwrap();
        stream.push(1, 100_000, &red).unwrap();
        stream.push(2, 200_000, &blue).unwrap();
        let summary = stream.finish().unwrap();
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.total_ms, 300);
        let info = crate::inspect_apng_file(&path).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 300);
        assert_eq!((info.width, info.height), (8, 8));
        assert!(!dir.join("out.tmp.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn svg_output_is_refused_by_the_frame_writers() {
        let dir = scratch("stream-svg");
        let path = dir.join("out.svg");
        let frame = solid([1, 2, 3]);
        let options = StreamOptions {
            format: OutputFormat::Svg,
            ..options(100)
        };
        let err = match stream_output(&path, &[frame], options) {
            Ok(_) => panic!("SVG output must be refused"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("tween route"));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
