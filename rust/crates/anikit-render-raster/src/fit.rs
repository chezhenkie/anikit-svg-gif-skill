//! Fitting stills to the canvas, Ken Burns windows and blends.

use anikit_core::{Error, Frame, Result};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};

const FILTER: FilterType = FilterType::Lanczos3;

/// How a still fills the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Cover,
    Contain,
}

/// The canvas size: the first image's aspect unless a height is given.
pub fn canvas_size(width: u32, height: Option<u32>, first: &Frame) -> Result<(u32, u32)> {
    if width < 8 {
        return Err(Error::invalid("width must be at least 8"));
    }
    let height = match height {
        Some(height) => height,
        None => {
            let aspect = first.height() as f64 / first.width() as f64;
            ((width as f64 * aspect).round() as u32).max(2)
        }
    };
    if height < 2 {
        return Err(Error::invalid("height must be at least 2"));
    }
    Ok((width, height))
}

/// Cover the canvas, cropping the overflow.
pub fn cover(still: &Frame, size: (u32, u32)) -> Result<Frame> {
    if still.size() == size {
        return Ok(still.clone());
    }
    let image = resize_to_fill(&to_rgba(still), size);
    from_rgba(image)
}

/// Fit inside the canvas, padding with the background colour.
pub fn contain(still: &Frame, size: (u32, u32), background: [u8; 3]) -> Result<Frame> {
    if still.size() == size {
        return Ok(still.clone());
    }
    let source = to_rgba(still);
    let scale = (size.0 as f64 / source.width() as f64).min(size.1 as f64 / source.height() as f64);
    let scaled_w = ((source.width() as f64 * scale).round() as u32).clamp(1, size.0);
    let scaled_h = ((source.height() as f64 * scale).round() as u32).clamp(1, size.1);
    let scaled = imageops::resize(&source, scaled_w, scaled_h, FILTER);
    let mut canvas = RgbaImage::from_pixel(
        size.0,
        size.1,
        Rgba([background[0], background[1], background[2], 255]),
    );
    let x = (size.0 - scaled.width()) / 2;
    let y = (size.1 - scaled.height()) / 2;
    imageops::replace(&mut canvas, &scaled, x as i64, y as i64);
    from_rgba(canvas)
}

/// The 2x base a Ken Burns window crops from, so the window never upscales.
pub fn zoom_base(still: &Frame, size: (u32, u32), fit: Fit, background: [u8; 3]) -> Result<Frame> {
    let prepared = match fit {
        Fit::Cover => to_rgba(still),
        Fit::Contain => to_rgba(&contain(still, size, background)?),
    };
    let base = resize_to_fill(&prepared, (size.0 * 2, size.1 * 2));
    from_rgba(base)
}

/// A centred window of the base at `zoom`, resized to the canvas.
pub fn zoom_window(base: &Frame, zoom: f64, size: (u32, u32)) -> Result<Frame> {
    if !zoom.is_finite() || zoom < 1.0 {
        return Err(Error::invalid("zoom must be 1.0 or more"));
    }
    let width = base.width() as f64 / zoom;
    let height = base.height() as f64 / zoom;
    let crop_w = (width.round() as u32).clamp(1, base.width());
    let crop_h = (height.round() as u32).clamp(1, base.height());
    let left = (base.width() - crop_w) / 2;
    let top = (base.height() - crop_h) / 2;
    let source = to_rgba(base);
    let window = imageops::crop_imm(&source, left, top, crop_w, crop_h).to_image();
    let image = imageops::resize(&window, size.0, size.1, FILTER);
    from_rgba(image)
}

/// Blend two frames of the same size, `mix` 0 keeping the first.
pub fn blend(first: &Frame, second: &Frame, mix: f32) -> Result<Frame> {
    if first.size() != second.size() {
        return Err(Error::invalid("cannot blend frames of different sizes"));
    }
    let mix = mix.clamp(0.0, 1.0);
    let mut out = Frame::new(first.width(), first.height())?;
    {
        let a = first.data();
        let b = second.data();
        let target = out.data_mut();
        for i in 0..target.len() {
            target[i] = (a[i] as f32 * (1.0 - mix) + b[i] as f32 * mix).round() as u8;
        }
    }
    Ok(out)
}

fn resize_to_fill(image: &RgbaImage, size: (u32, u32)) -> RgbaImage {
    let (width, height) = image.dimensions();
    let scale = (size.0 as f64 / width as f64).max(size.1 as f64 / height as f64);
    let scaled_w = ((width as f64 * scale).round() as u32).max(size.0);
    let scaled_h = ((height as f64 * scale).round() as u32).max(size.1);
    let scaled = imageops::resize(image, scaled_w, scaled_h, FILTER);
    let left = (scaled_w - size.0) / 2;
    let top = (scaled_h - size.1) / 2;
    imageops::crop_imm(&scaled, left, top, size.0, size.1).to_image()
}

fn to_rgba(frame: &Frame) -> RgbaImage {
    let (width, height) = frame.size();
    let mut image = RgbaImage::new(width, height);
    for (index, pixel) in image.pixels_mut().enumerate() {
        let at = index * 3;
        let data = frame.data();
        *pixel = Rgba([data[at], data[at + 1], data[at + 2], 255]);
    }
    image
}

fn from_rgba(image: RgbaImage) -> Result<Frame> {
    let (width, height) = image.dimensions();
    let raw = image.into_raw();
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    let (pixels, _) = raw.as_chunks::<4>();
    for pixel in pixels {
        rgb.extend_from_slice(&pixel[..3]);
    }
    Frame::from_rgb(width, height, rgb)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> Frame {
        let mut frame = Frame::new(width, height).unwrap();
        frame.fill(rgb);
        frame
    }

    #[test]
    fn the_height_follows_the_first_image() {
        let first = solid(400, 200, [0, 0, 0]);
        assert_eq!(canvas_size(800, None, &first).unwrap(), (800, 400));
        assert_eq!(canvas_size(800, Some(300), &first).unwrap(), (800, 300));
        assert!(canvas_size(4, None, &first).is_err());
    }

    #[test]
    fn cover_crops_and_keeps_the_size() {
        let wide = solid(8, 2, [255, 0, 0]);
        let out = cover(&wide, (4, 4)).unwrap();
        assert_eq!(out.size(), (4, 4));
        assert_eq!(out.pixel(0, 0), [255, 0, 0]);
    }

    #[test]
    fn a_same_size_still_is_not_filtered() {
        let mut still = Frame::new(4, 3).unwrap();
        for (index, pixel) in still
            .data_mut()
            .as_chunks_mut::<3>()
            .0
            .iter_mut()
            .enumerate()
        {
            *pixel = [(index * 37 % 251) as u8, (index * 53 % 241) as u8, 17];
        }
        assert_eq!(cover(&still, (4, 3)).unwrap().data(), still.data());
        assert_eq!(
            contain(&still, (4, 3), [9, 9, 9]).unwrap().data(),
            still.data()
        );
    }

    #[test]
    fn contain_pads_with_the_background() {
        let wide = solid(8, 2, [255, 0, 0]);
        let out = contain(&wide, (4, 4), [1, 2, 3]).unwrap();
        assert_eq!(out.size(), (4, 4));
        assert_eq!(out.pixel(0, 0), [1, 2, 3]);
        assert_eq!(out.pixel(2, 1), [255, 0, 0]);
    }

    #[test]
    fn a_zoom_window_crops_towards_the_centre() {
        let mut base = Frame::new(80, 40).unwrap();
        base.fill([0, 0, 0]);
        for y in 0..40 {
            for x in 20..60 {
                base.set_pixel(x, y, [255, 255, 255]);
            }
        }
        let out = zoom_window(&base, 2.0, (40, 20)).unwrap();
        assert_eq!(out.size(), (40, 20));
        assert_eq!(out.pixel(20, 10), [255, 255, 255]);
    }

    #[test]
    fn blending_moves_between_the_two_frames() {
        let a = solid(2, 2, [0, 0, 0]);
        let b = solid(2, 2, [200, 100, 40]);
        let half = blend(&a, &b, 0.5).unwrap();
        assert_eq!(half.pixel(0, 0), [100, 50, 20]);
        assert_eq!(blend(&a, &b, 0.0).unwrap().pixel(0, 0), [0, 0, 0]);
    }
}
