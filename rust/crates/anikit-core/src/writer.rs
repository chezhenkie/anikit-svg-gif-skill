//! Writing a GIF: validate, merge, palette, quantise, encode, verify, rename.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::encode::{GifEncoder, GifSummary};
use crate::inspect::inspect_file;
use crate::merge::{Run, merge_runs};
use crate::palette::Palette;
use crate::quantize::{Dither, Quantizer};
use crate::time::snap_delay;
use crate::{Error, Frame, Result};

/// Everything the writer needs that is not a frame.
#[derive(Debug, Clone)]
pub struct WriteOptions {
    pub loop_count: u16,
    pub colors: usize,
    /// How many frames the palette is sampled from, spread across the loop.
    pub palette_samples: usize,
    pub dither: Dither,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions {
            loop_count: 0,
            colors: 255,
            palette_samples: 12,
            dither: Dither::None,
        }
    }
}

/// Write a GIF and verify it, returning what was written.
///
/// Frames are quantised one at a time and encoded straight to disk, so the
/// animation never exists as a list of indexed frames.
pub fn write_gif(
    frames: &[Frame],
    delays: &[u32],
    path: &Path,
    options: &WriteOptions,
) -> Result<GifSummary> {
    if frames.is_empty() {
        return Err(Error::invalid("no frames to write"));
    }
    if frames.len() != delays.len() {
        return Err(Error::invalid(format!(
            "frame count {} and delay count {} differ",
            frames.len(),
            delays.len()
        )));
    }
    if !(2..=255).contains(&options.colors) {
        return Err(Error::invalid(format!(
            "colours must be between 2 and 255, got {}",
            options.colors
        )));
    }
    let (width, height) = frames[0].size();
    for frame in frames {
        if frame.size() != (width, height) {
            return Err(Error::invalid(format!(
                "frame size {:?} differs from {width}x{height}",
                frame.size()
            )));
        }
    }

    let snapped: Vec<u32> = delays.iter().map(|ms| snap_delay(*ms)).collect();
    let runs = merge_runs(frames, &snapped);
    let palette = sample_palette(frames, &runs, options.colors, options.palette_samples)?;
    let mut quantizer = Quantizer::new(palette.clone());

    let temp = temp_path(path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let result = write_and_verify(
        &mut quantizer,
        &palette,
        frames,
        &runs,
        options,
        &temp,
        path,
    );
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn write_and_verify(
    quantizer: &mut Quantizer,
    palette: &Palette,
    frames: &[Frame],
    runs: &[Run],
    options: &WriteOptions,
    temp: &Path,
    path: &Path,
) -> Result<GifSummary> {
    let (width, height) = frames[0].size();
    let file = File::create(temp)?;
    let mut encoder = GifEncoder::new(
        BufWriter::new(file),
        width,
        height,
        palette,
        options.loop_count,
    )?;
    for run in runs {
        let indices = quantizer.quantize(&frames[run.frame], options.dither);
        encoder.write_frame(&indices, run.delay_ms)?;
    }
    let summary = encoder.finish()?;
    verify_and_rename(temp, path, width, height, &summary)?;
    Ok(summary)
}

/// Check the file on disk against the summary, then move it into place.
pub(crate) fn verify_and_rename(
    temp: &Path,
    path: &Path,
    width: u32,
    height: u32,
    summary: &GifSummary,
) -> Result<()> {
    let info = inspect_file(temp)?;
    if info.frames != summary.frames {
        return Err(Error::verify(format!(
            "frame count {} does not match {}",
            info.frames, summary.frames
        )));
    }
    if info.width != width || info.height != height {
        return Err(Error::verify(format!(
            "size {}x{} does not match {width}x{height}",
            info.width, info.height
        )));
    }
    if info.total_ms.abs_diff(summary.total_ms) > 10 {
        return Err(Error::verify(format!(
            "duration {} ms does not match {} ms",
            info.total_ms, summary.total_ms
        )));
    }
    std::fs::rename(temp, path)?;
    Ok(())
}

fn sample_palette(frames: &[Frame], runs: &[Run], colors: usize, picks: usize) -> Result<Palette> {
    let picks = picks.max(1);
    let chosen: Vec<Frame> = if runs.len() <= picks {
        runs.iter().map(|run| frames[run.frame].clone()).collect()
    } else {
        (0..picks)
            .map(|i| {
                let index = ((i * (runs.len() - 1)) as f64 / (picks - 1) as f64).round() as usize;
                frames[runs[index].frame].clone()
            })
            .collect()
    };
    Palette::from_frames(&chosen, colors)
}

/// The temp file beside the target, e.g. `out.tmp.gif`.
pub(crate) fn temp_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("gif");
    path.with_file_name(format!("{stem}.tmp.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn writes_and_verifies_two_frames() {
        let dir = scratch("writer-test");
        let path = dir.join("two.gif");
        let frames = vec![solid([255, 0, 0]), solid([0, 0, 255])];
        let summary = write_gif(&frames, &[100, 100], &path, &WriteOptions::default()).unwrap();
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.total_ms, 200);
        assert_eq!(summary.width, 8);
        let info = inspect_file(&path).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 200);
        assert!(!dir.join("two.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_frames_merge() {
        let dir = scratch("writer-merge");
        let path = dir.join("hold.gif");
        let red = solid([255, 0, 0]);
        let frames = vec![red.clone(), red.clone(), solid([0, 255, 0])];
        let summary =
            write_gif(&frames, &[100, 100, 100], &path, &WriteOptions::default()).unwrap();
        assert_eq!(summary.frames, 2);
        assert_eq!(summary.total_ms, 300);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_frames_is_an_error_and_leaves_nothing() {
        let dir = scratch("writer-empty");
        let path = dir.join("empty.gif");
        assert!(write_gif(&[], &[], &path, &WriteOptions::default()).is_err());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
