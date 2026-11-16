//! The stills plan: holds, crossfades and Ken Burns windows, streamed out.
//!
//! Sources are decoded one frame at a time and finished frames are never held:
//! the peak is the canvas, the pair a crossfade blends and the twelve palette
//! samples. A GIF palette pass reads its sources a second time.

use std::path::{Path, PathBuf};

use anikit_core::{
    Dither, Error, Frame, FrameSink, MIN_DELAY_MS, OutputFormat, OutputSummary, Result,
    StreamOptions, check_canvas, check_frames, stream_output,
};
use anikit_time::FramePlan;

use crate::decode;
use crate::fit::{self, Fit};

/// How stills follow each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Sequence,
    Crossfade,
    KenBurns,
}

/// Everything the images route needs that is not a folder or an output path.
#[derive(Clone, Debug)]
pub struct Options {
    pub width: u32,
    pub height: Option<u32>,
    pub mode: Mode,
    pub fps: u32,
    pub hold: f64,
    pub transition: f64,
    pub zoom: f64,
    pub fit: Fit,
    pub bg: [u8; 3],
    pub colors: usize,
    pub dither: Dither,
    pub loop_count: u16,
}

/// What the route wrote, plus the first frame for an optional preview.
#[derive(Debug)]
pub struct Rendered {
    pub summary: OutputSummary,
    pub first: Frame,
}

/// Build one animation from a folder of images.
pub fn write_slideshow(folder: &Path, out: &Path, options: &Options) -> Result<Rendered> {
    validate(options)?;
    let format = OutputFormat::from_path(out)?;
    format.require_frames()?;
    let files = decode::scan_folder(folder, out)?;
    let first = decode::first_frame(&files, options.bg)?
        .ok_or_else(|| Error::invalid(format!("no images in {}", folder.display())))?;

    let size = fit::canvas_size(options.width, options.height, &first)?;
    check_canvas(size.0, size.1)?;
    let fps = options.fps as f64;
    let hold_count = ((options.hold * fps).round() as u32).max(1);
    let trans_count = if options.mode == Mode::Sequence {
        0
    } else {
        ((options.transition * fps).round()).max(0.0) as u32
    };
    let still_count = source_frame_count(&files, options.bg)?;
    let transitions = if still_count > 1 { trans_count } else { 0 };
    let per_image = hold_count + transitions;
    let count = still_count * per_image as u64;
    check_frames(count)?;
    let count = count as usize;
    let plan = FramePlan::uniform(count, fps)?;

    // The palette pass is GIF only; APNG stores truecolor frames (part 12).
    // It keeps the sampled frames and discards the rest.
    let slides = Slides {
        count: still_count,
        per_image,
        transitions,
        size,
    };

    let mut samples = Vec::new();
    if format == OutputFormat::Gif {
        let picks = picks(count);
        compose(&files, options, &slides, &first, &mut |index, frame, _| {
            if picks.contains(&index) {
                samples.push(frame.clone());
            }
            Ok(())
        })?;
    }

    let mut stream = stream_output(
        out,
        &samples,
        StreamOptions {
            format,
            width: size.0,
            height: size.1,
            loop_count: options.loop_count,
            colors: options.colors,
            dither: options.dither,
            total_ms: plan.total_ms(),
        },
    )?;

    let mut first_out: Option<Frame> = None;
    compose(
        &files,
        options,
        &slides,
        &first,
        &mut |index, frame, push| {
            if !push {
                return Ok(());
            }
            if first_out.is_none() {
                first_out = Some(frame.clone());
            }
            let pts = plan
                .presentation_us(index)
                .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
            stream.push(index as u32, pts, frame)
        },
    )?;
    let summary = stream.finish()?;
    let first = first_out.ok_or_else(|| Error::invalid("no frames were rendered"))?;
    Ok(Rendered { summary, first })
}

/// The palette sample indices, spread over the plan.
fn picks(count: usize) -> Vec<usize> {
    if count <= 12 {
        (0..count).collect()
    } else {
        (0..12)
            .map(|index| (index as f64 * (count - 1) as f64 / 11.0).round() as usize)
            .collect()
    }
}

/// Decoded stills across a folder, one frame at a time.
struct Stills<'a> {
    files: &'a [PathBuf],
    next_file: usize,
    current: Option<decode::Frames>,
    background: [u8; 3],
}

impl<'a> Stills<'a> {
    fn new(files: &'a [PathBuf], background: [u8; 3]) -> Stills<'a> {
        Stills {
            files,
            next_file: 0,
            current: None,
            background,
        }
    }
}

impl Iterator for Stills<'_> {
    type Item = Result<Frame>;

    fn next(&mut self) -> Option<Result<Frame>> {
        loop {
            if let Some(frames) = &mut self.current
                && let Some(frame) = frames.next()
            {
                return Some(frame);
            }
            if self.next_file >= self.files.len() {
                return None;
            }
            let path = &self.files[self.next_file];
            self.next_file += 1;
            match decode::Frames::new(path, self.background) {
                Ok(frames) => self.current = Some(frames),
                Err(error) => {
                    self.current = None;
                    return Some(Err(error));
                }
            }
        }
    }
}

/// How many source frames a folder holds, without keeping any of them.
fn source_frame_count(files: &[PathBuf], background: [u8; 3]) -> Result<u64> {
    let mut total = 0u64;
    for path in files {
        if decode::is_static(path) {
            total += 1;
            continue;
        }
        let mut frames = decode::Frames::new(path, background)?;
        for frame in frames.by_ref() {
            frame?;
            total += 1;
        }
    }
    Ok(total)
}

/// The shape of a slideshow plan: frames per still, the transition length and
/// the canvas it renders into.
struct Slides {
    count: u64,
    per_image: u32,
    transitions: u32,
    size: (u32, u32),
}

/// Emit every conceptual output frame in order, holding only the pair a
/// transition needs.
fn compose(
    files: &[PathBuf],
    options: &Options,
    slides: &Slides,
    first: &Frame,
    sink: &mut impl FnMut(usize, &Frame, bool) -> Result<()>,
) -> Result<()> {
    let hold_count = slides.per_image - slides.transitions;
    let scene = Scene {
        options,
        size: slides.size,
        hold_count,
    };
    let mut stills = Stills::new(files, options.bg);
    // The first still is passed in, so the iterator starts after it.
    let _ = stills.next();
    let mut current = first.clone();
    let mut index = 0usize;
    for image in 0..slides.count {
        let next = if image + 1 < slides.count {
            Some(
                stills
                    .next()
                    .transpose()?
                    .ok_or_else(|| Error::invalid("an image disappeared while it was read"))?,
            )
        } else if slides.transitions > 0 {
            Some(first.clone())
        } else {
            None
        };
        let image = image as usize;
        let base = scene.base(&current)?;
        let motion = direction(image);
        match &base {
            Base::Still(frame) => {
                sink(index, frame, true)?;
                for offset in 1..hold_count as usize {
                    sink(index + offset, frame, false)?;
                }
                index += hold_count as usize;
            }
            Base::Zoom(_) => {
                for k in 0..hold_count {
                    let frame = scene.frame(&base, k, motion)?;
                    sink(index, &frame, true)?;
                    index += 1;
                }
            }
        }
        if let Some(next) = &next {
            let next_base = scene.base(next)?;
            let last = scene.frame(&base, hold_count - 1, motion)?;
            let first_next = scene.frame(
                &next_base,
                0,
                direction((image + 1) % slides.count as usize),
            )?;
            for j in 0..slides.transitions {
                let mix = (j + 1) as f32 / (slides.transitions + 1) as f32;
                let frame = fit::blend(&last, &first_next, mix)?;
                sink(index, &frame, true)?;
                index += 1;
            }
        }
        if let Some(next) = next {
            current = next;
        }
    }
    debug_assert_eq!(index as u64, slides.count * slides.per_image as u64);
    Ok(())
}

/// Build one animation from stills that hold for their recorded delays.
///
/// This is the rebuild side of part 13: no frame grid and no transition, only
/// the per-frame delays of an extract manifest. Delays arrive in folder order.
pub fn write_timed(
    folder: &Path,
    out: &Path,
    delays: &[u32],
    options: &Options,
) -> Result<Rendered> {
    if options.width < 8 {
        return Err(Error::invalid("width must be at least 8"));
    }
    check_canvas(options.width, options.height.unwrap_or(options.width))?;
    let format = OutputFormat::from_path(out)?;
    format.require_frames()?;
    let files = decode::scan_folder(folder, out)?;
    let count = source_frame_count(&files, options.bg)?;
    if count != delays.len() as u64 {
        return Err(Error::invalid(format!(
            "the timing has {} delays for {count} images",
            delays.len()
        )));
    }
    let first = decode::first_frame(&files, options.bg)?
        .ok_or_else(|| Error::invalid(format!("no images in {}", folder.display())))?;
    let size = fit::canvas_size(options.width, options.height, &first)?;
    check_canvas(size.0, size.1)?;
    // The format floor applies on rebuild: a source delay of 0 or 10 ms is
    // written as 20 ms, and the total grows with it.
    let raised: Vec<u32> = delays
        .iter()
        .map(|delay| (*delay).max(MIN_DELAY_MS))
        .collect();
    let total_ms = raised.iter().try_fold(0u32, |sum, delay| {
        sum.checked_add(*delay)
            .ok_or_else(|| Error::invalid("the delays add up past the 32-bit range"))
    })?;

    // The palette pass is GIF only, as on every route (part 03).
    let mut samples = Vec::new();
    if format == OutputFormat::Gif {
        let picks: Vec<usize> = if count <= 12 {
            (0..count as usize).collect()
        } else {
            (0..12)
                .map(|index| index * (count as usize - 1) / 11)
                .collect()
        };
        for (seen, still) in Stills::new(&files, options.bg).enumerate() {
            let still = still?;
            if picks.contains(&seen) {
                samples.push(fit_still(&still, size, options)?);
            }
        }
    }

    let mut stream = stream_output(
        out,
        &samples,
        StreamOptions {
            format,
            width: size.0,
            height: size.1,
            loop_count: options.loop_count,
            colors: options.colors,
            dither: options.dither,
            total_ms,
        },
    )?;

    let mut first_out: Option<Frame> = None;
    let mut pts_ms = 0u64;
    for (seen, still) in Stills::new(&files, options.bg).enumerate() {
        let frame = fit_still(&still?, size, options)?;
        if first_out.is_none() {
            first_out = Some(frame.clone());
        }
        stream.push(seen as u32, pts_ms * 1000, &frame)?;
        pts_ms += raised[seen] as u64;
    }
    let summary = stream.finish()?;
    let first = first_out.ok_or_else(|| Error::invalid("no frames were rendered"))?;
    Ok(Rendered { summary, first })
}

fn fit_still(still: &Frame, size: (u32, u32), options: &Options) -> Result<Frame> {
    match options.fit {
        Fit::Cover => fit::cover(still, size),
        Fit::Contain => fit::contain(still, size, options.bg),
    }
}

fn validate(options: &Options) -> Result<()> {
    if !(1..=50).contains(&options.fps) {
        return Err(Error::invalid("fps must be between 1 and 50"));
    }
    if options.width < 8 {
        return Err(Error::invalid("width must be at least 8"));
    }
    check_canvas(options.width, options.height.unwrap_or(options.width))?;
    if !options.hold.is_finite() || options.hold <= 0.0 {
        return Err(Error::invalid("hold must be greater than 0"));
    }
    if !options.zoom.is_finite() || options.zoom < 1.0 {
        return Err(Error::invalid("zoom must be 1.0 or more"));
    }
    Ok(())
}

fn direction(image: usize) -> i32 {
    if image % 2 == 1 { -1 } else { 1 }
}

/// A prepared source image: a fitted still or a 2x Ken Burns base.
enum Base {
    Still(Frame),
    Zoom(Frame),
}

struct Scene<'a> {
    options: &'a Options,
    size: (u32, u32),
    hold_count: u32,
}

impl Scene<'_> {
    fn base(&self, still: &Frame) -> Result<Base> {
        if self.options.mode == Mode::KenBurns {
            return Ok(Base::Zoom(fit::zoom_base(
                still,
                self.size,
                self.options.fit,
                self.options.bg,
            )?));
        }
        let frame = match self.options.fit {
            Fit::Cover => fit::cover(still, self.size)?,
            Fit::Contain => fit::contain(still, self.size, self.options.bg)?,
        };
        Ok(Base::Still(frame))
    }

    fn frame(&self, base: &Base, k: u32, direction: i32) -> Result<Frame> {
        match base {
            Base::Still(frame) => Ok(frame.clone()),
            Base::Zoom(base) => {
                let t = if self.hold_count <= 1 {
                    0.0
                } else {
                    k as f64 / (self.hold_count - 1) as f64
                };
                let t = if direction < 0 { 1.0 - t } else { t };
                let zoom = 1.0 + (self.options.zoom - 1.0) * t;
                fit::zoom_window(base, zoom, self.size)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anikit_core::inspect_file;
    use image::{Rgba, RgbaImage};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/slideshow")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(path: &Path, width: u32, height: u32, rgb: [u8; 3]) {
        let image = RgbaImage::from_pixel(width, height, Rgba([rgb[0], rgb[1], rgb[2], 255]));
        image.save(path).unwrap();
    }

    /// A two-axis gradient, so every zoom level changes the pixels.
    fn gradient_png(path: &Path, width: u32, height: u32, shift: u32) {
        let mut image = RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let r = ((x * 255 / width.max(1)) + shift) % 256;
            let g = (y * 255 / height.max(1)) as u8;
            *pixel = Rgba([r as u8, g, 128, 255]);
        }
        image.save(path).unwrap();
    }

    fn options(mode: Mode) -> Options {
        Options {
            width: 40,
            height: Some(20),
            mode,
            fps: 10,
            hold: 0.5,
            transition: 0.2,
            zoom: 1.2,
            fit: Fit::Cover,
            bg: [0, 0, 0],
            colors: 255,
            dither: Dither::None,
            loop_count: 0,
        }
    }

    #[test]
    fn sequence_holds_each_image_once() {
        let dir = scratch("sequence");
        png(&dir.join("1.png"), 8, 8, [255, 0, 0]);
        png(&dir.join("2.png"), 8, 8, [0, 0, 255]);
        let out = dir.join("out.gif");
        let result = write_slideshow(&dir, &out, &options(Mode::Sequence)).unwrap();
        assert_eq!(result.summary.frames, 2);
        assert_eq!(result.summary.total_ms, 1000);
        assert_eq!((result.summary.width, result.summary.height), (40, 20));
        let info = inspect_file(&out).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 1000);
        let _ = std::fs::remove_dir_all(&dir);
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

    #[test]
    fn timed_stills_hold_for_their_recorded_delays() {
        let dir = scratch("timed");
        png(&dir.join("1.png"), 8, 8, [255, 0, 0]);
        png(&dir.join("2.png"), 8, 8, [0, 0, 255]);
        let out = dir.join("out.gif");
        let result = write_timed(&dir, &out, &[100, 250], &options(Mode::Sequence)).unwrap();
        assert_eq!(result.summary.frames, 2);
        assert_eq!(result.summary.total_ms, 350);
        assert_eq!(delays(&out), vec![100, 250]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_zero_delay_rebuilds_at_the_floor() {
        let dir = scratch("timed-floor");
        png(&dir.join("1.png"), 8, 8, [255, 0, 0]);
        png(&dir.join("2.png"), 8, 8, [0, 0, 255]);
        let out = dir.join("out.gif");
        let result = write_timed(&dir, &out, &[0, 10], &options(Mode::Sequence)).unwrap();
        assert_eq!(result.summary.total_ms, 40);
        assert_eq!(delays(&out), vec![20, 20]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_timing_count_mismatch_is_refused() {
        let dir = scratch("timed-mismatch");
        png(&dir.join("1.png"), 8, 8, [255, 0, 0]);
        png(&dir.join("2.png"), 8, 8, [0, 0, 255]);
        let out = dir.join("out.gif");
        let err = write_timed(&dir, &out, &[100], &options(Mode::Sequence)).unwrap_err();
        assert!(err.to_string().contains("2 images"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crossfade_adds_blended_frames() {
        let dir = scratch("crossfade");
        png(&dir.join("1.png"), 8, 8, [255, 0, 0]);
        png(&dir.join("2.png"), 8, 8, [0, 0, 255]);
        let out = dir.join("out.gif");
        let result = write_slideshow(&dir, &out, &options(Mode::Crossfade)).unwrap();
        // Two holds and two blends per image; the plan is fourteen frames.
        assert_eq!(result.summary.frames, 6);
        assert_eq!(result.summary.total_ms, 1400);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_image_has_no_transitions() {
        let dir = scratch("single");
        png(&dir.join("only.png"), 8, 8, [10, 20, 30]);
        let out = dir.join("out.gif");
        let result = write_slideshow(&dir, &out, &options(Mode::Crossfade)).unwrap();
        assert_eq!(result.summary.frames, 1);
        assert_eq!(result.summary.total_ms, 500);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn kenburns_changes_the_picture_within_a_hold() {
        let dir = scratch("kenburns");
        gradient_png(&dir.join("1.png"), 40, 20, 0);
        gradient_png(&dir.join("2.png"), 40, 20, 90);
        let out = dir.join("out.gif");
        let mut opts = options(Mode::KenBurns);
        opts.zoom = 1.5;
        let result = write_slideshow(&dir, &out, &opts).unwrap();
        assert_eq!(result.summary.frames, 14);
        assert_eq!(result.summary.total_ms, 1400);
        let bytes = std::fs::read(&out).unwrap();
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let mut first: Option<Vec<u8>> = None;
        let mut different = false;
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            let buffer = frame.buffer.to_vec();
            match &first {
                None => first = Some(buffer),
                Some(seen) => different |= seen != &buffer,
            }
        }
        assert!(different, "kenburns frames must move");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_fails() {
        let dir = scratch("missing");
        let err = write_slideshow(
            &dir.join("nope"),
            &dir.join("o.gif"),
            &options(Mode::Sequence),
        )
        .unwrap_err();
        assert!(err.to_string().contains("folder not found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_options_are_rejected() {
        let dir = scratch("bad-options");
        let mut opts = options(Mode::Sequence);
        opts.fps = 0;
        assert!(write_slideshow(&dir, &dir.join("o.gif"), &opts).is_err());
        opts = options(Mode::Sequence);
        opts.width = 4;
        assert!(write_slideshow(&dir, &dir.join("o.gif"), &opts).is_err());
        opts = options(Mode::Sequence);
        opts.hold = 0.0;
        assert!(write_slideshow(&dir, &dir.join("o.gif"), &opts).is_err());
        opts = options(Mode::Sequence);
        opts.zoom = 0.5;
        assert!(write_slideshow(&dir, &dir.join("o.gif"), &opts).is_err());
        opts = options(Mode::Sequence);
        opts.width = 9000;
        assert!(write_slideshow(&dir, &dir.join("o.gif"), &opts).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
