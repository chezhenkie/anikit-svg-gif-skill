//! Reading a GIF or APNG back into numbered PNG frames and a timing manifest.
//!
//! This is the reverse of the two frame writers (part 13). A source animation
//! is decoded one frame at a time, matted onto the background, written as a
//! PNG, and read back before its entry in the manifest counts. The whole
//! folder is assembled under a temp name and renamed only when every frame and
//! the manifest agree.

use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anikit_core::{Error, Frame, Result};
use image::codecs::gif::GifDecoder;
use image::metadata::LoopCount;
use image::{AnimationDecoder, ImageDecoder};
use png::{BitDepth, BlendOp, ColorType, Decoder, DisposeOp, Transformations};
use serde_json::{Map, Value as Json};

use crate::decode;

/// A source format the extract route reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    Gif,
    Apng,
}

impl SourceFormat {
    pub fn name(self) -> &'static str {
        match self {
            SourceFormat::Gif => "gif",
            SourceFormat::Apng => "apng",
        }
    }

    fn from_name(text: &str) -> Option<SourceFormat> {
        match text {
            "gif" => Some(SourceFormat::Gif),
            "apng" => Some(SourceFormat::Apng),
            _ => None,
        }
    }
}

/// What extraction produced, for the summary line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtractInfo {
    pub frames: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
}

/// The timing file beside the frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub format: SourceFormat,
    pub width: u32,
    pub height: u32,
    pub loop_count: u16,
    pub delays: Vec<u32>,
}

impl Manifest {
    /// The exact text written to `manifest.json`.
    pub fn text(&self) -> String {
        let delays: Vec<String> = self.delays.iter().map(u32::to_string).collect();
        format!(
            "{{\n  \"version\": 1,\n  \"format\": \"{}\",\n  \"width\": {},\n  \"height\": {},\n  \"loop\": {},\n  \"delays\": [{}]\n}}\n",
            self.format.name(),
            self.width,
            self.height,
            self.loop_count,
            delays.join(", ")
        )
    }

    /// The summed presentation time of the frames.
    pub fn total_ms(&self) -> Result<u32> {
        self.delays.iter().try_fold(0u32, |sum, delay| {
            sum.checked_add(*delay)
                .ok_or_else(|| Error::invalid("the manifest delays add up past the 32-bit range"))
        })
    }
}

/// Read one frame file's name for a one-based index.
pub fn frame_name(index: u32) -> String {
    format!("frame-{index:04}.png")
}

/// Turn one animation into a folder of PNG frames and a manifest.
pub fn extract(input: &Path, folder: &Path, background: [u8; 3]) -> Result<ExtractInfo> {
    let format = source_format(input)?;
    if !input.is_file() {
        return Err(Error::invalid(format!(
            "input not found: {}",
            input.display()
        )));
    }
    if folder.exists() {
        return Err(Error::invalid(format!(
            "output folder already exists: {}",
            folder.display()
        )));
    }
    let name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error::invalid(format!(
                "cannot name the output folder {}",
                folder.display()
            ))
        })?;
    if let Some(parent) = folder
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| Error::io(format!("cannot create {}: {err}", parent.display())))?;
    }
    let temp = folder.with_file_name(format!("{name}.tmp"));
    if temp.exists() {
        return Err(Error::invalid(format!(
            "temp folder already exists: {}",
            temp.display()
        )));
    }
    fs::create_dir(&temp)
        .map_err(|err| Error::io(format!("cannot create {}: {err}", temp.display())))?;
    let mut guard = TempFolder {
        path: temp.clone(),
        keep: false,
    };

    let mut writer = FolderWriter {
        folder: &temp,
        frames: 0,
    };
    let source = match format {
        SourceFormat::Gif => read_gif(input, background, &mut writer)?,
        SourceFormat::Apng => read_apng(input, background, &mut writer)?,
    };
    if source.delays.is_empty() {
        return Err(Error::invalid(format!(
            "{} has no frames to extract",
            input.display()
        )));
    }
    let manifest = Manifest {
        format,
        width: source.width,
        height: source.height,
        loop_count: source.loop_count,
        delays: source.delays,
    };
    let total_ms = manifest.total_ms()?;
    let text = manifest.text();
    let path = temp.join("manifest.json");
    fs::write(&path, text.as_bytes())
        .map_err(|err| Error::io(format!("cannot write {}: {err}", path.display())))?;
    let back = fs::read(&path)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
    if back != text.as_bytes() {
        return Err(Error::verify(format!(
            "{} does not read back",
            path.display()
        )));
    }
    fs::rename(&temp, folder)
        .map_err(|err| Error::io(format!("cannot rename {}: {err}", temp.display())))?;
    guard.keep = true;

    Ok(ExtractInfo {
        frames: writer.frames,
        total_ms,
        width: manifest.width,
        height: manifest.height,
    })
}

/// Read a manifest written by `extract`.
pub fn read_manifest(path: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(path)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
    let value: Json = serde_json::from_str(&text)
        .map_err(|err| Error::invalid(format!("cannot read {}: {err}", path.display())))?;
    let Some(object) = value.as_object() else {
        return Err(Error::invalid(format!(
            "{} must hold a JSON object",
            path.display()
        )));
    };
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "version" | "format" | "width" | "height" | "loop" | "delays"
        ) {
            return Err(Error::invalid(format!(
                "unknown field {key} in {}",
                path.display()
            )));
        }
    }
    if integer(object, "version", path)? != 1 {
        return Err(Error::invalid(format!(
            "{} has a version other than 1",
            path.display()
        )));
    }
    let format_text = object
        .get("format")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Error::invalid(format!("missing field format in {}", path.display())))?;
    let format = SourceFormat::from_name(format_text).ok_or_else(|| {
        Error::invalid(format!(
            "field format in {} must be gif or apng",
            path.display()
        ))
    })?;
    let width = dimension(object, "width", path)?;
    let height = dimension(object, "height", path)?;
    let plays = integer(object, "loop", path)?;
    if !(0..=65535).contains(&plays) {
        return Err(Error::invalid(format!(
            "field loop in {} is out of range",
            path.display()
        )));
    }
    let Some(items) = object.get("delays").and_then(|value| value.as_array()) else {
        return Err(Error::invalid(format!(
            "missing field delays in {}",
            path.display()
        )));
    };
    if items.is_empty() {
        return Err(Error::invalid(format!(
            "field delays in {} is empty",
            path.display()
        )));
    }
    let mut delays = Vec::with_capacity(items.len());
    for item in items {
        let delay = item.as_u64().filter(|value| *value <= u32::MAX as u64);
        let Some(delay) = delay else {
            return Err(Error::invalid(format!(
                "field delays in {} must hold non-negative integers",
                path.display()
            )));
        };
        delays.push(delay as u32);
    }
    Ok(Manifest {
        format,
        width,
        height,
        loop_count: plays as u16,
        delays,
    })
}

fn integer(object: &Map<String, Json>, key: &str, path: &Path) -> Result<i64> {
    let value = object
        .get(key)
        .ok_or_else(|| Error::invalid(format!("missing field {key} in {}", path.display())))?;
    value.as_i64().ok_or_else(|| {
        Error::invalid(format!(
            "field {key} in {} must be an integer",
            path.display()
        ))
    })
}

fn dimension(object: &Map<String, Json>, key: &str, path: &Path) -> Result<u32> {
    let value = integer(object, key, path)?;
    if !(1..=65535).contains(&value) {
        return Err(Error::invalid(format!(
            "field {key} in {} is out of range",
            path.display()
        )));
    }
    Ok(value as u32)
}

fn source_format(path: &Path) -> Result<SourceFormat> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("gif") => Ok(SourceFormat::Gif),
        Some("png") | Some("apng") => Ok(SourceFormat::Apng),
        _ => Err(Error::invalid("input must end in .gif, .png or .apng")),
    }
}

/// A folder that removes itself unless the extraction committed.
struct TempFolder {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct Source {
    width: u32,
    height: u32,
    loop_count: u16,
    delays: Vec<u32>,
}

/// One PNG per frame, written and read back as the decoder yields them.
struct FolderWriter<'a> {
    folder: &'a Path,
    frames: u32,
}

impl FolderWriter<'_> {
    fn frame(&mut self, frame: &Frame) -> Result<()> {
        self.frames += 1;
        let path = self.folder.join(frame_name(self.frames));
        write_png(&path, frame)
    }
}

fn read_gif(input: &Path, background: [u8; 3], writer: &mut FolderWriter) -> Result<Source> {
    let file = File::open(input)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", input.display())))?;
    let decoder = GifDecoder::new(BufReader::new(file))
        .map_err(|err| Error::invalid(format!("cannot read {}: {err}", input.display())))?;
    let (width, height) = decoder.dimensions();
    let loop_count = match decoder.loop_count() {
        LoopCount::Infinite => 0,
        LoopCount::Finite(count) => count.get().min(u16::MAX as u32) as u16,
    };
    let mut delays = Vec::new();
    let mut matted = Frame::new(width, height)?;
    for frame in decoder.into_frames() {
        let frame = frame
            .map_err(|err| Error::invalid(format!("cannot decode {}: {err}", input.display())))?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = if denominator == 0 {
            0
        } else {
            (numerator as u64 + denominator as u64 / 2) / denominator as u64
        };
        let delay = u32::try_from(delay).unwrap_or(u32::MAX);
        decode::matte_into(frame.buffer(), background, &mut matted);
        writer.frame(&matted)?;
        delays.push(delay);
    }
    Ok(Source {
        width,
        height,
        loop_count,
        delays,
    })
}

fn read_apng(input: &Path, background: [u8; 3], writer: &mut FolderWriter) -> Result<Source> {
    let file = File::open(input)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", input.display())))?;
    let mut decoder = Decoder::new(BufReader::new(file));
    decoder.set_transformations(Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|err| Error::invalid(format!("cannot read {}: {err}", input.display())))?;
    let (width, height, num_frames, num_plays, default_has_control) = {
        let info = reader.info();
        let Some(control) = info.animation_control() else {
            return Err(Error::invalid(format!(
                "{} is not an animated PNG",
                input.display()
            )));
        };
        (
            info.width,
            info.height,
            control.num_frames,
            control.num_plays,
            info.frame_control.is_some(),
        )
    };
    if num_frames == 0 {
        return Err(Error::invalid(format!(
            "{} names no animation frames",
            input.display()
        )));
    }
    let (color, depth) = reader.output_color_type();
    if depth != BitDepth::Eight {
        return Err(Error::invalid(format!(
            "{} did not decode to 8-bit samples",
            input.display()
        )));
    }
    let mut buffer = vec![0u8; width as usize * height as usize * 4];
    if !default_has_control {
        reader
            .next_frame(&mut buffer)
            .map_err(|err| decode_error(input, err))?;
    }

    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    let mut matted = Frame::new(width, height)?;
    let mut delays = Vec::with_capacity(num_frames as usize);
    let mut disposal: Option<Disposal> = None;
    for _ in 0..num_frames {
        let output = reader
            .next_frame(&mut buffer)
            .map_err(|err| decode_error(input, err))?;
        let control = reader.info().frame_control.ok_or_else(|| {
            Error::verify(format!(
                "{} has a frame without a control chunk",
                input.display()
            ))
        })?;
        let rect = Rect {
            x: control.x_offset,
            y: control.y_offset,
            width: output.width,
            height: output.height,
        };
        rect.check(width, height, input)?;
        if let Some(previous) = disposal.take() {
            previous.apply(&mut canvas, width);
        }
        let backup = if control.dispose_op == DisposeOp::Previous {
            Some(copy_rect(&canvas, width, rect))
        } else {
            None
        };
        blend(
            &mut canvas,
            width,
            &buffer,
            output.line_size,
            color,
            control.blend_op,
            rect,
        )?;
        matte_canvas(&canvas, background, &mut matted);
        writer.frame(&matted)?;
        delays.push(delay_ms(control.delay_num, control.delay_den));
        disposal = Some(Disposal {
            rect,
            op: control.dispose_op,
            backup,
        });
    }

    Ok(Source {
        width,
        height,
        loop_count: num_plays.min(u16::MAX as u32) as u16,
        delays,
    })
}

fn decode_error(input: &Path, err: png::DecodingError) -> Error {
    Error::invalid(format!("cannot decode {}: {err}", input.display()))
}

fn delay_ms(numerator: u16, denominator: u16) -> u32 {
    let denominator = if denominator == 0 { 100 } else { denominator };
    let ms = (numerator as u64 * 1000 + denominator as u64 / 2) / denominator as u64;
    u32::try_from(ms).unwrap_or(u32::MAX)
}

#[derive(Clone, Copy)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl Rect {
    fn check(self, width: u32, height: u32, input: &Path) -> Result<()> {
        if self.width == 0
            || self.height == 0
            || self.x.saturating_add(self.width) > width
            || self.y.saturating_add(self.height) > height
        {
            return Err(Error::verify(format!(
                "a frame in {} leaves the canvas",
                input.display()
            )));
        }
        Ok(())
    }
}

struct Disposal {
    rect: Rect,
    op: DisposeOp,
    backup: Option<Vec<u8>>,
}

impl Disposal {
    fn apply(self, canvas: &mut [u8], width: u32) {
        match self.op {
            DisposeOp::None => {}
            DisposeOp::Background => {
                for row in 0..self.rect.height as usize {
                    let start =
                        ((self.rect.y as usize + row) * width as usize + self.rect.x as usize) * 4;
                    let end = start + self.rect.width as usize * 4;
                    canvas[start..end].fill(0);
                }
            }
            DisposeOp::Previous => {
                if let Some(backup) = self.backup {
                    restore_rect(canvas, width, self.rect, &backup);
                }
            }
        }
    }
}

fn copy_rect(canvas: &[u8], width: u32, rect: Rect) -> Vec<u8> {
    let mut out = Vec::with_capacity(rect.width as usize * rect.height as usize * 4);
    for row in 0..rect.height as usize {
        let start = ((rect.y as usize + row) * width as usize + rect.x as usize) * 4;
        let end = start + rect.width as usize * 4;
        out.extend_from_slice(&canvas[start..end]);
    }
    out
}

fn restore_rect(canvas: &mut [u8], width: u32, rect: Rect, backup: &[u8]) {
    for row in 0..rect.height as usize {
        let start = ((rect.y as usize + row) * width as usize + rect.x as usize) * 4;
        let end = start + rect.width as usize * 4;
        let from = row * rect.width as usize * 4;
        canvas[start..end].copy_from_slice(&backup[from..from + rect.width as usize * 4]);
    }
}

fn blend(
    canvas: &mut [u8],
    width: u32,
    source: &[u8],
    stride: usize,
    color: ColorType,
    op: BlendOp,
    rect: Rect,
) -> Result<()> {
    let channels = match color {
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Indexed => {
            return Err(Error::invalid(
                "an indexed APNG frame did not expand to RGB",
            ));
        }
    };
    for row in 0..rect.height as usize {
        for column in 0..rect.width as usize {
            let at = row * stride + column * channels;
            let pixel = match channels {
                4 => [source[at], source[at + 1], source[at + 2], source[at + 3]],
                3 => [source[at], source[at + 1], source[at + 2], 255],
                2 => [source[at], source[at], source[at], source[at + 1]],
                _ => [source[at], source[at], source[at], 255],
            };
            let target = ((rect.y as usize + row) * width as usize + rect.x as usize + column) * 4;
            match op {
                BlendOp::Source => canvas[target..target + 4].copy_from_slice(&pixel),
                BlendOp::Over => alpha_over(&mut canvas[target..target + 4], pixel),
            }
        }
    }
    Ok(())
}

fn alpha_over(target: &mut [u8], source: [u8; 4]) {
    let alpha = source[3] as u32;
    if alpha == 0 {
        return;
    }
    if alpha == 255 {
        target.copy_from_slice(&source);
        return;
    }
    let base = target[3] as u32;
    let rest = 255 - alpha;
    let out_alpha = alpha + (base * rest + 127) / 255;
    if out_alpha == 0 {
        target.fill(0);
        return;
    }
    let denominator = out_alpha * 255;
    for channel in 0..3 {
        let numerator = source[channel] as u32 * alpha * 255
            + target[channel] as u32 * base * rest
            + denominator / 2;
        target[channel] = (numerator / denominator).min(255) as u8;
    }
    target[3] = out_alpha as u8;
}

fn matte_canvas(canvas: &[u8], background: [u8; 3], frame: &mut Frame) {
    let data = frame.data_mut();
    for (index, pixel) in canvas.as_chunks::<4>().0.iter().enumerate() {
        let alpha = pixel[3] as u32;
        let keep = 255 - alpha;
        let at = index * 3;
        for channel in 0..3 {
            data[at + channel] =
                ((pixel[channel] as u32 * alpha + background[channel] as u32 * keep + 127) / 255)
                    as u8;
        }
    }
}

/// Write one frame as an 8-bit RGB PNG and decode it again.
fn write_png(path: &Path, frame: &Frame) -> Result<()> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, frame.width(), frame.height());
        encoder.set_color(ColorType::Rgb);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|err| Error::io(format!("cannot start {}: {err}", path.display())))?;
        writer
            .write_image_data(frame.data())
            .map_err(|err| Error::io(format!("cannot write {}: {err}", path.display())))?;
        writer
            .finish()
            .map_err(|err| Error::io(format!("cannot finish {}: {err}", path.display())))?;
    }
    fs::write(path, &bytes)
        .map_err(|err| Error::io(format!("cannot write {}: {err}", path.display())))?;
    let back = read_png(path)?;
    if back.size() != frame.size() || back.data() != frame.data() {
        return Err(Error::verify(format!(
            "{} does not read back as written",
            path.display()
        )));
    }
    Ok(())
}

fn read_png(path: &Path) -> Result<Frame> {
    let file = File::open(path)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
    let decoder = Decoder::new(BufReader::new(file));
    let mut reader = decoder
        .read_info()
        .map_err(|err| Error::invalid(format!("cannot read {}: {err}", path.display())))?;
    let (color, depth) = reader.output_color_type();
    if color != ColorType::Rgb || depth != BitDepth::Eight {
        return Err(Error::verify(format!(
            "{} is not an 8-bit RGB PNG",
            path.display()
        )));
    }
    let mut buffer = vec![0u8; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|err| Error::invalid(format!("cannot decode {}: {err}", path.display())))?;
    buffer.truncate(info.width as usize * info.height as usize * 3);
    Frame::from_rgb(info.width, info.height, buffer)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use anikit_core::{Dither, FrameSink, OutputFormat, StreamOptions, stream_output};
    use image::{Rgba, RgbaImage};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/extract")
            .join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn solid(rgb: [u8; 3]) -> Frame {
        let mut frame = Frame::new(8, 8).unwrap();
        frame.fill(rgb);
        frame
    }

    fn apng_source(path: &Path) {
        let red = solid([255, 0, 0]);
        let blue = solid([0, 0, 255]);
        let options = StreamOptions {
            format: OutputFormat::Apng,
            width: 8,
            height: 8,
            loop_count: 2,
            colors: 255,
            dither: Dither::None,
            total_ms: 300,
        };
        let mut stream = stream_output(path, &[], options).unwrap();
        stream.push(0, 0, &red).unwrap();
        stream.push(1, 100_000, &red).unwrap();
        stream.push(2, 200_000, &blue).unwrap();
        stream.finish().unwrap();
    }

    #[test]
    fn an_apng_extracts_to_frames_and_a_manifest() {
        let dir = scratch("apng");
        let source = dir.join("source.apng");
        apng_source(&source);
        let folder = dir.join("frames");
        let info = extract(&source, &folder, [0, 0, 0]).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 300);
        assert_eq!((info.width, info.height), (8, 8));
        assert!(!dir.join("frames.tmp").exists());

        let first = read_png(&folder.join("frame-0001.png")).unwrap();
        assert_eq!(first.pixel(0, 0), [255, 0, 0]);
        let second = read_png(&folder.join("frame-0002.png")).unwrap();
        assert_eq!(second.pixel(7, 7), [0, 0, 255]);

        let manifest = read_manifest(&folder.join("manifest.json")).unwrap();
        assert_eq!(manifest.format, SourceFormat::Apng);
        assert_eq!(manifest.delays, vec![200, 100]);
        assert_eq!(manifest.loop_count, 2);
        assert_eq!(manifest.width, 8);
        assert_eq!(manifest.total_ms().unwrap(), 300);
    }

    fn gif_source(path: &Path) {
        let palette = [0, 0, 0, 255, 0, 0, 0, 0, 255];
        let mut bytes = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut bytes, 4, 4, &palette).unwrap();
            encoder.set_repeat(gif::Repeat::Finite(1)).unwrap();
            let first = gif::Frame {
                width: 2,
                height: 2,
                left: 1,
                top: 1,
                delay: 10,
                transparent: Some(0),
                buffer: Cow::Owned(vec![1, 1, 1, 1]),
                ..Default::default()
            };
            encoder.write_frame(&first).unwrap();

            let second = gif::Frame {
                width: 2,
                height: 2,
                left: 0,
                top: 0,
                delay: 25,
                transparent: Some(0),
                buffer: Cow::Owned(vec![2, 2, 2, 2]),
                ..Default::default()
            };
            encoder.write_frame(&second).unwrap();
        }
        fs::write(path, &bytes).unwrap();
    }

    #[test]
    fn a_gif_extracts_its_composited_frames() {
        let dir = scratch("gif");
        let source = dir.join("source.gif");
        gif_source(&source);
        let folder = dir.join("frames");
        let info = extract(&source, &folder, [0, 255, 0]).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 350);

        let first = read_png(&folder.join("frame-0001.png")).unwrap();
        assert_eq!(first.pixel(1, 1), [255, 0, 0]);
        assert_eq!(first.pixel(0, 0), [0, 255, 0]);

        let second = read_png(&folder.join("frame-0002.png")).unwrap();
        assert_eq!(second.pixel(0, 0), [0, 0, 255]);
        // The kept region of the first frame stays behind the second.
        assert_eq!(second.pixel(2, 2), [255, 0, 0]);

        let manifest = read_manifest(&folder.join("manifest.json")).unwrap();
        assert_eq!(manifest.format, SourceFormat::Gif);
        assert_eq!(manifest.delays, vec![100, 250]);
        assert_eq!(manifest.loop_count, 1);
    }

    #[test]
    fn the_output_folder_is_never_overwritten() {
        let dir = scratch("exists");
        let source = dir.join("source.apng");
        apng_source(&source);
        let folder = dir.join("frames");
        fs::create_dir(&folder).unwrap();
        let err = extract(&source, &folder, [0, 0, 0]).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_still_png_and_a_foreign_extension_are_refused() {
        let dir = scratch("refused");
        let image = RgbaImage::from_pixel(2, 2, Rgba([1, 2, 3, 255]));
        image.save(dir.join("still.png")).unwrap();
        let err = extract(&dir.join("still.png"), &dir.join("frames"), [0, 0, 0]).unwrap_err();
        assert!(err.to_string().contains("not an animated PNG"));
        assert!(!dir.join("frames").exists());

        fs::write(dir.join("clip.webp"), b"nothing").unwrap();
        let err = extract(&dir.join("clip.webp"), &dir.join("frames"), [0, 0, 0]).unwrap_err();
        assert!(err.to_string().contains("input must end in"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_rejects_bad_fields() {
        let dir = scratch("manifest");
        let path = dir.join("manifest.json");
        fs::write(
            &path,
            "{\"version\":1,\"format\":\"gif\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[20]}",
        )
        .unwrap();
        assert!(read_manifest(&path).is_ok());

        fs::write(
            &path,
            "{\"version\":2,\"format\":\"gif\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[20]}",
        )
        .unwrap();
        assert!(read_manifest(&path).is_err());

        fs::write(
            &path,
            "{\"version\":1,\"format\":\"webp\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[20]}",
        )
        .unwrap();
        assert!(read_manifest(&path).is_err());

        fs::write(
            &path,
            "{\"version\":1,\"format\":\"gif\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[]}",
        )
        .unwrap();
        assert!(read_manifest(&path).is_err());

        fs::write(
            &path,
            "{\"version\":1,\"format\":\"gif\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[20],\"extra\":1}",
        )
        .unwrap();
        assert!(read_manifest(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
