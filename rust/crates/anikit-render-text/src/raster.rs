//! Filling flattened contours into a coverage mask.
//!
//! Each pixel row is sampled on eight horizontal scan lines; every span is
//! covered exactly in x. That gives smooth edges at glyph sizes without the
//! cost of a full supersample per pixel.

use crate::outline::Contour;

const SUBSAMPLES: u32 = 8;

/// An 8-bit coverage mask placed relative to the pen origin at the baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

impl Mask {
    /// Coverage at a pixel inside the mask, 0 outside it.
    pub fn alpha_at(&self, x: i32, y: i32) -> u8 {
        let px = x - self.left;
        let py = y - self.top;
        if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
            return 0;
        }
        self.alpha[py as usize * self.width as usize + px as usize]
    }
}

/// Rasterise closed polygons into a mask, or None when they enclose no area.
pub fn rasterize(contours: &[Contour]) -> Option<Mask> {
    let mut edges = Vec::new();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for contour in contours {
        if contour.len() < 3 {
            continue;
        }
        for i in 0..contour.len() {
            let a = contour[i];
            let b = contour[(i + 1) % contour.len()];
            if !a[0].is_finite() || !a[1].is_finite() || !b[0].is_finite() || !b[1].is_finite() {
                continue;
            }
            min_x = min_x.min(a[0]);
            min_y = min_y.min(a[1]);
            max_x = max_x.max(a[0]);
            max_y = max_y.max(a[1]);
            if a[1] == b[1] {
                continue;
            }
            let dir = if b[1] > a[1] { 1 } else { -1 };
            edges.push((a[0], a[1], b[0], b[1], dir));
        }
    }
    if edges.is_empty() {
        return None;
    }

    let left = min_x.floor() as i32;
    let top = min_y.floor() as i32;
    let width = (max_x.ceil() as i32 - left).max(1) as u32;
    let height = (max_y.ceil() as i32 - top).max(1) as u32;
    let mut alpha = vec![0u8; width as usize * height as usize];
    let mut row = vec![0f32; width as usize];
    let mut crossings: Vec<(f32, i32)> = Vec::new();

    for py in 0..height {
        row.fill(0.0);
        for sub in 0..SUBSAMPLES {
            let y = top as f32 + py as f32 + (sub as f32 + 0.5) / SUBSAMPLES as f32;
            crossings.clear();
            for edge in &edges {
                let (x0, y0, x1, y1, dir) = *edge;
                let inside = (y0 <= y && y < y1) || (y1 <= y && y < y0);
                if !inside {
                    continue;
                }
                let t = (y - y0) / (y1 - y0);
                crossings.push((x0 + t * (x1 - x0), dir));
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut winding = 0;
            let mut span_start = 0.0f32;
            for (x, dir) in crossings.iter().copied() {
                let before = winding;
                winding += dir;
                if before == 0 && winding != 0 {
                    span_start = x;
                } else if before != 0 && winding == 0 {
                    add_span(&mut row, span_start, x, left);
                }
            }
        }
        let scale = 255.0 / SUBSAMPLES as f32;
        for (px, value) in row.iter().enumerate() {
            alpha[py as usize * width as usize + px] =
                (value * scale).round().clamp(0.0, 255.0) as u8;
        }
    }

    Some(Mask {
        left,
        top,
        width,
        height,
        alpha,
    })
}

fn add_span(row: &mut [f32], from: f32, to: f32, left: i32) {
    let mut a = from - left as f32;
    let mut b = to - left as f32;
    if b < a {
        std::mem::swap(&mut a, &mut b);
    }
    a = a.max(0.0);
    b = b.min(row.len() as f32);
    if b <= a {
        return;
    }
    let first = a.floor() as usize;
    let last = (b.ceil() as usize).min(row.len());
    for (index, cell) in row.iter_mut().enumerate().take(last).skip(first) {
        let low = a.max(index as f32);
        let high = b.min(index as f32 + 1.0);
        if high > low {
            *cell += high - low;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Contour {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn a_square_pixel_gets_full_coverage() {
        let mask = rasterize(&[square(0.0, 0.0, 4.0, 4.0)]).unwrap();
        assert_eq!((mask.left, mask.top), (0, 0));
        assert_eq!((mask.width, mask.height), (4, 4));
        assert!(mask.alpha.iter().all(|a| *a == 255));
    }

    #[test]
    fn half_a_pixel_gets_half_coverage() {
        let mask = rasterize(&[square(0.0, 0.0, 4.5, 4.0)]).unwrap();
        for y in 0..4 {
            assert_eq!(mask.alpha_at(4, y), 128);
            assert_eq!(mask.alpha_at(0, y), 255);
        }
    }

    #[test]
    fn a_hole_stays_empty() {
        let contours = vec![
            square(0.0, 0.0, 8.0, 8.0),
            square(2.0, 2.0, 6.0, 6.0).into_iter().rev().collect(),
        ];
        let mask = rasterize(&contours).unwrap();
        assert_eq!(mask.alpha_at(4, 4), 0);
        assert_eq!(mask.alpha_at(0, 0), 255);
    }

    #[test]
    fn empty_input_has_no_mask() {
        assert!(rasterize(&[]).is_none());
        assert!(rasterize(&[vec![[0.0, 0.0], [1.0, 0.0]]]).is_none());
    }
}
