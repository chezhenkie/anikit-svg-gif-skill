//! Turning image files into opaque RGB frames.
//!
//! Every decoded frame is EXIF-rotated and matted onto the background before
//! it reaches the layout code, so alpha never survives into a GIF (09, row 10).
//! Animated GIF and WebP sources yield their frames in order, one at a time.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anikit_core::{Error, Frame, Result};
use image::{AnimationDecoder, DynamicImage, ImageFormat, RgbaImage};

const EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"];

/// Image files in a folder, naturally sorted, with dot files and the output
/// itself left out.
pub fn scan_folder(folder: &Path, skip: &Path) -> Result<Vec<PathBuf>> {
    if !folder.is_dir() {
        return Err(Error::invalid(format!(
            "folder not found: {}",
            folder.display()
        )));
    }
    let skip = skip.canonicalize().ok();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder)
        .map_err(|err| Error::io(format!("cannot list {}: {err}", folder.display())))?
    {
        let entry =
            entry.map_err(|err| Error::io(format!("cannot list {}: {err}", folder.display())))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let image_file = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                let lower = ext.to_ascii_lowercase();
                EXTENSIONS.contains(&lower.as_str())
            })
            .unwrap_or(false);
        if !image_file {
            continue;
        }
        if skip.is_some() && path.canonicalize().ok() == skip {
            continue;
        }
        files.push(path);
    }
    files.sort_by_cached_key(|path| {
        natural_key(
            &path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    });
    if files.is_empty() {
        return Err(Error::invalid(format!("no images in {}", folder.display())));
    }
    Ok(files)
}

/// Decode one file into every frame it carries, for the tests.
#[cfg(test)]
pub fn load(path: &Path, background: [u8; 3]) -> Result<Vec<Frame>> {
    Frames::new(path, background)?.collect()
}

/// A file whose first frame is the whole image.
pub fn is_static(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff")
    )
}

/// The first frame of the first file that has one.
pub fn first_frame(files: &[PathBuf], background: [u8; 3]) -> Result<Option<Frame>> {
    for file in files {
        let mut frames = Frames::new(file, background)?;
        if let Some(frame) = frames.next() {
            return frame.map(Some);
        }
    }
    Ok(None)
}

/// Frames from one file, decoded one at a time.
///
/// An animated GIF or WebP yields every frame in order; any other file yields
/// one. The caller holds one frame; the decoder keeps its own buffers.
pub struct Frames {
    inner: Inner,
    path: PathBuf,
    background: [u8; 3],
    orientation: u32,
}

enum Inner {
    Animated(image::Frames<'static>),
    Still(Option<Frame>),
}

impl Frames {
    pub fn new(path: &Path, background: [u8; 3]) -> Result<Frames> {
        let orientation = orientation(path);
        let reader = image::ImageReader::open(path)
            .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?
            .with_guessed_format()
            .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
        let format = reader.format();

        let mut frames = Frames {
            inner: Inner::Still(None),
            path: path.to_path_buf(),
            background,
            orientation,
        };
        if format == Some(ImageFormat::Gif) {
            let file = File::open(path)
                .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
            let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(file))
                .map_err(|err| decode_error(path, err))?;
            frames.inner = Inner::Animated(decoder.into_frames());
            return Ok(frames);
        }
        if format == Some(ImageFormat::WebP) {
            let file = File::open(path)
                .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
            let decoder = image::codecs::webp::WebPDecoder::new(BufReader::new(file))
                .map_err(|err| decode_error(path, err))?;
            if decoder.has_animation() {
                frames.inner = Inner::Animated(decoder.into_frames());
                return Ok(frames);
            }
        }

        let image = reader.decode().map_err(|err| decode_error(path, err))?;
        let frame = matte(rotate(image, orientation).to_rgba8(), background)?;
        frames.inner = Inner::Still(Some(frame));
        Ok(frames)
    }
}

impl Iterator for Frames {
    type Item = Result<Frame>;

    fn next(&mut self) -> Option<Result<Frame>> {
        match &mut self.inner {
            Inner::Animated(frames) => frames.next().map(|frame| {
                let frame = frame.map_err(|err| decode_error(&self.path, err))?;
                let image = DynamicImage::ImageRgba8(frame.into_buffer());
                matte(rotate(image, self.orientation).to_rgba8(), self.background)
            }),
            Inner::Still(slot) => slot.take().map(Ok),
        }
    }
}

fn decode_error(path: &Path, err: image::ImageError) -> Error {
    Error::invalid(format!("cannot decode {}: {err}", path.display()))
}

fn orientation(path: &Path) -> u32 {
    let Ok(file) = File::open(path) else {
        return 1;
    };
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return 1;
    };
    exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
        .and_then(|field| field.value.get_uint(0))
        .filter(|value| (1..=8).contains(value))
        .unwrap_or(1)
}

/// Apply an EXIF orientation tag, the same mapping Pillow uses.
fn rotate(image: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn matte(image: RgbaImage, background: [u8; 3]) -> Result<Frame> {
    let (width, height) = image.dimensions();
    let mut frame = Frame::new(width, height)?;
    matte_into(&image, background, &mut frame);
    Ok(frame)
}

/// Matted RGBA pixels written into an existing frame, so a decoder loop can
/// reuse one buffer for every frame.
pub(crate) fn matte_into(image: &RgbaImage, background: [u8; 3], frame: &mut Frame) {
    debug_assert_eq!(frame.size(), image.dimensions());
    let data = frame.data_mut();
    for (index, pixel) in image.pixels().enumerate() {
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Text(String),
}

/// Split a name into number and text runs, numbers first on a tie.
fn natural_key(name: &str) -> Vec<Part> {
    let lower = name.to_lowercase();
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = false;
    for ch in lower.chars() {
        let is_digit = ch.is_ascii_digit();
        if !current.is_empty() && is_digit != digits {
            parts.push(if digits {
                Part::Number(current.parse().unwrap_or(u64::MAX))
            } else {
                Part::Text(std::mem::take(&mut current))
            });
            current.clear();
        }
        digits = is_digit;
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(if digits {
            Part::Number(current.parse().unwrap_or(u64::MAX))
        } else {
            Part::Text(current)
        });
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/images")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(path: &Path, width: u32, height: u32, colour: [u8; 3]) {
        let image =
            RgbaImage::from_pixel(width, height, Rgba([colour[0], colour[1], colour[2], 255]));
        image.save(path).unwrap();
    }

    #[test]
    fn natural_order_puts_two_before_ten() {
        let mut names = vec!["10.png", "2.png", "1.png", "b.png", "a.png"];
        names.sort_by_cached_key(|name| natural_key(name));
        assert_eq!(names, vec!["1.png", "2.png", "10.png", "a.png", "b.png"]);
    }

    #[test]
    fn a_png_decodes_to_its_pixels() {
        let dir = scratch("decode-png");
        let path = dir.join("a.png");
        write_png(&path, 3, 2, [200, 100, 50]);
        let frames = load(&path, [0, 0, 0]).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].size(), (3, 2));
        assert_eq!(frames[0].pixel(1, 1), [200, 100, 50]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alpha_is_matted_onto_the_background() {
        let dir = scratch("decode-alpha");
        let path = dir.join("a.png");
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 128]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 0]));
        image.save(&path).unwrap();
        let frames = load(&path, [10, 20, 30]).unwrap();
        let left = frames[0].pixel(0, 0);
        assert!(left[0] > 120 && left[0] < 140, "left {left:?}");
        assert_eq!(frames[0].pixel(1, 0), [10, 20, 30]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exif_rotation_is_applied() {
        let dir = scratch("decode-exif");
        let path = dir.join("rotated.jpg");
        let mut image = RgbaImage::from_pixel(4, 2, Rgba([0, 0, 255, 255]));
        for y in 0..2 {
            for x in 0..2 {
                image.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let rgb = DynamicImage::ImageRgba8(image).to_rgb8();
        let mut buffer = std::io::Cursor::new(Vec::new());
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut buffer);
        encoder.encode_image(&DynamicImage::ImageRgb8(rgb)).unwrap();
        let mut bytes = buffer.into_inner();
        let exif = minimal_exif(6);
        insert_exif(&mut bytes, &exif);
        std::fs::write(&path, &bytes).unwrap();

        let frames = load(&path, [0, 0, 0]).unwrap();
        assert_eq!(frames[0].size(), (2, 4));
        assert!(frames[0].pixel(0, 0)[0] > 200, "top should be red");
        assert!(frames[0].pixel(0, 3)[2] > 200, "bottom should be blue");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_lists_images_and_skips_the_output() {
        let dir = scratch("decode-folder");
        write_png(&dir.join("1.png"), 2, 2, [1, 2, 3]);
        write_png(&dir.join("10.png"), 2, 2, [1, 2, 3]);
        write_png(&dir.join("2.png"), 2, 2, [1, 2, 3]);
        std::fs::write(dir.join("notes.txt"), b"skip").unwrap();
        let files = scan_folder(&dir, &dir.join("2.png")).unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["1.png", "10.png"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_folder_is_an_error() {
        let dir = scratch("decode-empty");
        assert!(scan_folder(&dir, &dir.join("out.gif")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A minimal EXIF block with only an orientation tag, in TIFF byte order.
    fn minimal_exif(orientation: u16) -> Vec<u8> {
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"MM\x00\x2a");
        tiff.extend_from_slice(&8u32.to_be_bytes());
        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend_from_slice(&0x0112u16.to_be_bytes());
        tiff.extend_from_slice(&3u16.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&0u16.to_be_bytes());
        tiff.extend_from_slice(&0u32.to_be_bytes());
        tiff
    }

    /// Insert an APP1 EXIF segment after the SOI marker.
    fn insert_exif(jpeg: &mut Vec<u8>, tiff: &[u8]) {
        let mut segment = Vec::new();
        segment.extend_from_slice(b"Exif\x00\x00");
        segment.extend_from_slice(tiff);
        let length = (segment.len() + 2) as u16;
        let mut insert = Vec::new();
        insert.extend_from_slice(&[0xFF, 0xE1]);
        insert.extend_from_slice(&length.to_be_bytes());
        insert.extend_from_slice(&segment);
        jpeg.splice(2..2, insert);
    }
}
