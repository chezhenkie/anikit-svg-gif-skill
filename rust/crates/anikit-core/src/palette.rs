//! One palette for the whole loop, built by median cut over a 5-bit histogram.
//!
//! The histogram keeps 32 levels per channel, which is the usual trade for a
//! median cut: fine enough that flat areas do not band, small enough that the
//! split loop stays cheap on a large frame count.

use crate::{Error, Frame, Result};

const BIN_COUNT: usize = 32 * 32 * 32;
const SHIFT: u32 = 3;

/// The colours a frame is mapped onto. Never more than 255 entries.
#[derive(Clone, PartialEq, Eq)]
pub struct Palette {
    colors: Vec<[u8; 3]>,
}

impl Palette {
    /// Build one palette from a set of frames, typically sampled across the loop.
    pub fn from_frames(frames: &[Frame], colors: usize) -> Result<Palette> {
        if frames.is_empty() {
            return Err(Error::invalid("no frames to sample a palette from"));
        }
        if !(2..=255).contains(&colors) {
            return Err(Error::invalid(format!(
                "palette size must be between 2 and 255, got {colors}"
            )));
        }

        let mut count = vec![0u32; BIN_COUNT];
        let mut sums = vec![[0u64; 3]; BIN_COUNT];
        for frame in frames {
            let (pixels, _) = frame.data().as_chunks::<3>();
            for px in pixels {
                let bin = bin_index(px[0], px[1], px[2]);
                count[bin] += 1;
                sums[bin][0] += px[0] as u64;
                sums[bin][1] += px[1] as u64;
                sums[bin][2] += px[2] as u64;
            }
        }

        let mut boxes = vec![Extent {
            lo: [0, 0, 0],
            hi: [31, 31, 31],
        }];
        while boxes.len() < colors {
            let mut best: Option<(usize, u32, Extent, Extent)> = None;
            for (i, ext) in boxes.iter().enumerate() {
                let Some(axis) = widest_axis(ext) else {
                    continue;
                };
                let Some((left, right)) = split(&count, ext, axis) else {
                    continue;
                };
                let n = count_in(&count, ext);
                if n > 0 && best.is_none_or(|(_, most, _, _)| n > most) {
                    best = Some((i, n, left, right));
                }
            }
            let Some((i, _, left, right)) = best else {
                break;
            };
            boxes[i] = left;
            boxes.push(right);
        }

        let mut palette = Vec::with_capacity(boxes.len());
        for ext in &boxes {
            let (n, sum) = stats(&count, &sums, ext);
            if n == 0 {
                continue;
            }
            let n = n as u64;
            palette.push([
                ((sum[0] + n / 2) / n) as u8,
                ((sum[1] + n / 2) / n) as u8,
                ((sum[2] + n / 2) / n) as u8,
            ]);
        }
        if palette.is_empty() {
            return Err(Error::invalid("palette came out empty"));
        }
        Ok(Palette { colors: palette })
    }

    /// A palette from an explicit list, for tests and for exact fixtures.
    pub fn from_colors(colors: Vec<[u8; 3]>) -> Result<Palette> {
        if colors.is_empty() || colors.len() > 255 {
            return Err(Error::invalid(
                "palette must hold between 1 and 255 colours",
            ));
        }
        Ok(Palette { colors })
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    pub fn colors(&self) -> &[[u8; 3]] {
        &self.colors
    }
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Palette {} colours", self.colors.len())
    }
}

#[derive(Clone, Copy)]
struct Extent {
    lo: [u8; 3],
    hi: [u8; 3],
}

fn bin_index(r: u8, g: u8, b: u8) -> usize {
    ((r as usize >> SHIFT) << 10) | ((g as usize >> SHIFT) << 5) | (b as usize >> SHIFT)
}

fn widest_axis(ext: &Extent) -> Option<usize> {
    let mut axis = None;
    let mut widest = 0u8;
    for a in 0..3 {
        let width = ext.hi[a] - ext.lo[a];
        if width > widest {
            widest = width;
            axis = Some(a);
        }
    }
    axis
}

fn bins(ext: &Extent) -> impl Iterator<Item = usize> + '_ {
    (ext.lo[0]..=ext.hi[0]).flat_map(move |r| {
        (ext.lo[1]..=ext.hi[1]).flat_map(move |g| {
            (ext.lo[2]..=ext.hi[2])
                .map(move |b| ((r as usize) << 10) | ((g as usize) << 5) | b as usize)
        })
    })
}

fn count_in(count: &[u32], ext: &Extent) -> u32 {
    bins(ext).map(|bin| count[bin]).sum()
}

fn stats(count: &[u32], sums: &[[u64; 3]], ext: &Extent) -> (u32, [u64; 3]) {
    let mut n = 0u32;
    let mut sum = [0u64; 3];
    for bin in bins(ext) {
        n += count[bin];
        sum[0] += sums[bin][0];
        sum[1] += sums[bin][1];
        sum[2] += sums[bin][2];
    }
    (n, sum)
}

fn split(count: &[u32], ext: &Extent, axis: usize) -> Option<(Extent, Extent)> {
    let total = count_in(count, ext);
    if total < 2 {
        return None;
    }
    // Cut at the boundary nearest half the mass, but never at one that leaves
    // a side empty. A cut on the heavy bin itself would trap it with the
    // colours below and never separate them.
    let mut seen = 0u32;
    let mut best: Option<(u8, u64)> = None;
    for v in ext.lo[axis]..ext.hi[axis] {
        let mut slice = *ext;
        slice.lo[axis] = v;
        slice.hi[axis] = v;
        seen += count_in(count, &slice);
        if seen == 0 || seen == total {
            continue;
        }
        let distance = (2 * seen as i64 - total as i64).unsigned_abs();
        if best.is_none_or(|(_, closest)| distance < closest) {
            best = Some((v, distance));
        }
    }
    let (cut, _) = best?;
    let mut left = *ext;
    left.hi[axis] = cut;
    let mut right = *ext;
    right.lo[axis] = cut + 1;
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_of(colours: &[[u8; 3]], repeat: usize) -> Frame {
        let mut frame = Frame::new(colours.len() as u32 * repeat as u32, 1).unwrap();
        for (i, c) in colours.iter().enumerate() {
            for r in 0..repeat {
                frame.set_pixel((i * repeat + r) as u32, 0, *c);
            }
        }
        frame
    }

    #[test]
    fn two_colours_are_kept_exactly() {
        let frame = frame_of(&[[255, 0, 0], [0, 0, 255]], 8);
        let palette = Palette::from_frames(&[frame], 16).unwrap();
        assert!(palette.len() >= 2);
        let has_red = palette.colors().contains(&[255, 0, 0]);
        let has_blue = palette.colors().contains(&[0, 0, 255]);
        assert!(has_red && has_blue, "colours {:?}", palette.colors());
    }

    #[test]
    fn palette_size_is_capped() {
        let frame = frame_of(&[[10, 10, 10], [200, 200, 200]], 4);
        let palette = Palette::from_frames(&[frame], 2).unwrap();
        assert_eq!(palette.len(), 2);
    }

    #[test]
    fn a_dominant_background_does_not_blend_light_colours() {
        let bg = [10, 13, 18];
        let fg = [230, 237, 246];
        let accent = [17, 168, 205];
        let mut frame = Frame::new(64, 48).unwrap();
        frame.fill(bg);
        for x in 0..16 {
            frame.set_pixel(x, 0, fg);
            frame.set_pixel(x, 1, accent);
        }
        let palette = Palette::from_frames(&[frame], 255).unwrap();
        let colors = palette.colors();
        assert!(colors.contains(&bg), "{colors:?}");
        assert!(colors.contains(&fg), "{colors:?}");
        assert!(colors.contains(&accent), "{colors:?}");
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(Palette::from_frames(&[], 16).is_err());
    }
}
