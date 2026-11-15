//! Mapping RGB frames onto one palette, with optional Floyd-Steinberg dithering.

use crate::{Frame, Palette};

const BIN_COUNT: usize = 32 * 32 * 32;

/// How a frame is mapped onto the palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    #[default]
    None,
    Floyd,
}

/// Nearest-colour lookup with a cache over the 5-bit RGB space.
///
/// The cache holds one byte per 5-bit colour, so a frame of flat areas asks the
/// palette once per distinct colour instead of once per pixel.
pub struct Quantizer {
    palette: Palette,
    cache: Vec<u8>,
}

impl Quantizer {
    pub fn new(palette: Palette) -> Quantizer {
        Quantizer {
            palette,
            cache: vec![0; BIN_COUNT],
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The palette index closest to one colour.
    pub fn index_of(&mut self, rgb: [u8; 3]) -> u8 {
        let key =
            ((rgb[0] as usize >> 3) << 10) | ((rgb[1] as usize >> 3) << 5) | (rgb[2] as usize >> 3);
        let cached = self.cache[key];
        if cached != 0 {
            return cached - 1;
        }
        let mut best = 0u8;
        let mut best_d = u32::MAX;
        for (i, c) in self.palette.colors().iter().enumerate() {
            let dr = rgb[0] as i32 - c[0] as i32;
            let dg = rgb[1] as i32 - c[1] as i32;
            let db = rgb[2] as i32 - c[2] as i32;
            let d = (dr * dr + dg * dg + db * db) as u32;
            if d < best_d {
                best_d = d;
                best = i as u8;
                if d == 0 {
                    break;
                }
            }
        }
        self.cache[key] = best + 1;
        best
    }

    /// One frame as palette indices, in row order.
    pub fn quantize(&mut self, frame: &Frame, dither: Dither) -> Vec<u8> {
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let mut out = vec![0u8; width * height];
        match dither {
            Dither::None => {
                let (pixels, _) = frame.data().as_chunks::<3>();
                for (i, px) in pixels.iter().enumerate() {
                    out[i] = self.index_of([px[0], px[1], px[2]]);
                }
            }
            Dither::Floyd => self.floyd(frame, &mut out),
        }
        out
    }

    fn floyd(&mut self, frame: &Frame, out: &mut [u8]) {
        let width = frame.width() as usize;
        let height = frame.height() as usize;
        let stride = (width + 2) * 3;
        let mut current = vec![0i32; stride];
        let mut next = vec![0i32; stride];
        for y in 0..height {
            for x in 0..width {
                let px = frame.pixel(x as u32, y as u32);
                let e = (x + 1) * 3;
                let r = clamp(px[0] as i32 + current[e] / 16);
                let g = clamp(px[1] as i32 + current[e + 1] / 16);
                let b = clamp(px[2] as i32 + current[e + 2] / 16);
                let index = self.index_of([r, g, b]);
                out[y * width + x] = index;
                let c = self.palette.colors()[index as usize];
                let err = [
                    r as i32 - c[0] as i32,
                    g as i32 - c[1] as i32,
                    b as i32 - c[2] as i32,
                ];
                for (channel, value) in err.iter().enumerate() {
                    current[e + 3 + channel] += value * 7;
                    next[e - 3 + channel] += value * 3;
                    next[e + channel] += value * 5;
                    next[e + 3 + channel] += value;
                }
            }
            std::mem::swap(&mut current, &mut next);
            next.fill(0);
        }
    }
}

fn clamp(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgb: [u8; 3]) -> Frame {
        let mut frame = Frame::new(4, 4).unwrap();
        frame.fill(rgb);
        frame
    }

    #[test]
    fn exact_colours_map_exactly() {
        let palette = Palette::from_colors(vec![[255, 0, 0], [0, 0, 255]]).unwrap();
        let mut q = Quantizer::new(palette);
        assert_eq!(q.index_of([255, 0, 0]), 0);
        assert_eq!(q.index_of([0, 0, 255]), 1);
    }

    #[test]
    fn solid_frames_use_one_index() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut q = Quantizer::new(palette);
        let indices = q.quantize(&solid([255, 255, 255]), Dither::None);
        assert!(indices.iter().all(|i| *i == 1));
    }

    #[test]
    fn dithering_mixes_a_gradient() {
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut frame = Frame::new(16, 4).unwrap();
        for x in 0..16 {
            for y in 0..4 {
                frame.set_pixel(x, y, [128, 128, 128]);
            }
        }
        let mut q = Quantizer::new(palette);
        let plain = q.quantize(&frame, Dither::None);
        assert!(plain.iter().all(|i| *i == plain[0]));
        let dithered = q.quantize(&frame, Dither::Floyd);
        assert!(dithered.contains(&0) && dithered.contains(&1));
    }
}
