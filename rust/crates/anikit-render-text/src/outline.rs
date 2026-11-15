//! Flattening glyph outlines into pixel-space polygons.

use anikit_core::Result;

use crate::font::Face;

/// One closed polygon in pixels, y down, baseline at y zero.
pub type Contour = Vec<[f32; 2]>;

/// Flatten every contour of a glyph at a pixel size.
pub fn flatten(face: &Face, gid: u16, size_px: u16) -> Result<Vec<Contour>> {
    let units = face.glyph_points(gid)?;
    Ok(flatten_points(
        &units,
        size_px as f32 / face.units_per_em() as f32,
    ))
}

/// Flatten font-unit contours at a scale, flipping the y axis.
fn flatten_points(contours: &[Vec<crate::font::Point>], scale: f32) -> Vec<Contour> {
    let mut out = Vec::with_capacity(contours.len());
    for contour in contours {
        if let Some(polygon) = flatten_one(contour, scale) {
            out.push(polygon);
        }
    }
    out
}

fn flatten_one(contour: &[crate::font::Point], scale: f32) -> Option<Contour> {
    if contour.len() < 2 {
        return None;
    }
    let n = contour.len();
    let mut ring = Vec::with_capacity(n + n / 2);
    for i in 0..n {
        ring.push(contour[i]);
        let next = contour[(i + 1) % n];
        if !contour[i].on && !next.on {
            ring.push(crate::font::Point {
                x: (contour[i].x + next.x) * 0.5,
                y: (contour[i].y + next.y) * 0.5,
                on: true,
            });
        }
    }
    let first_on = ring.iter().position(|p| p.on)?;
    ring.rotate_left(first_on);

    let m = ring.len();
    let mut out: Contour = Vec::with_capacity(m);
    out.push(to_pixel(ring[0], scale));
    let mut start = ring[0];
    let mut i = 1;
    while i < m {
        let point = ring[i];
        if point.on {
            push_point(&mut out, to_pixel(point, scale));
            start = point;
            i += 1;
        } else {
            let next = if i + 1 < m { ring[i + 1] } else { ring[0] };
            let control = to_pixel(point, scale);
            let end = to_pixel(next, scale);
            flatten_quad(&mut out, to_pixel(start, scale), control, end);
            start = next;
            i += 2;
        }
    }
    if out.len() >= 3 { Some(out) } else { None }
}

fn flatten_quad(out: &mut Contour, start: [f32; 2], control: [f32; 2], end: [f32; 2]) {
    let bend = {
        let dx = start[0] - 2.0 * control[0] + end[0];
        let dy = start[1] - 2.0 * control[1] + end[1];
        (dx * dx + dy * dy).sqrt()
    };
    let steps = ((bend / 0.25).sqrt().ceil() as usize).clamp(2, 64);
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let u = 1.0 - t;
        let x = u * u * start[0] + 2.0 * u * t * control[0] + t * t * end[0];
        let y = u * u * start[1] + 2.0 * u * t * control[1] + t * t * end[1];
        push_point(out, [x, y]);
    }
}

fn to_pixel(point: crate::font::Point, scale: f32) -> [f32; 2] {
    [point.x * scale, -point.y * scale]
}

fn push_point(out: &mut Contour, point: [f32; 2]) {
    if let Some(last) = out.last()
        && (last[0] - point[0]).abs() < 0.01
        && (last[1] - point[1]).abs() < 0.01
    {
        return;
    }
    out.push(point);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m_flattens_inside_the_line_box() {
        let face = Face::embedded();
        let metrics = face.metrics(28).unwrap();
        let contours = flatten(face, face.glyph_index('M').unwrap(), 28).unwrap();
        assert!(!contours.is_empty());
        for contour in &contours {
            assert!(contour.len() >= 4);
            for point in contour {
                assert!(point[0] >= -2.0 && point[0] <= metrics.advance as f32 + 2.0);
                assert!(point[1] >= -(metrics.ascent as f32 + 2.0));
                assert!(point[1] <= metrics.descent as f32 + 2.0);
            }
        }
    }

    #[test]
    fn curves_produce_more_points_than_corners() {
        let face = Face::embedded();
        let o = flatten(face, face.glyph_index('o').unwrap(), 28).unwrap();
        let total: usize = o.iter().map(|c| c.len()).sum();
        assert!(total > 16);
    }
}
