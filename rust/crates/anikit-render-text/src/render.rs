//! Drawing parsed cells onto a frame, and placing the grid on the canvas.

use std::collections::HashMap;

use anikit_core::{Error, Frame, Result};

use crate::TextFrame;
use crate::font::{Face, Metrics};
use crate::outline;
use crate::raster::{self, Mask};

/// Where the text grid sits inside the output frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub cols: usize,
    pub rows: usize,
    ox: i32,
    oy: i32,
}

impl Layout {
    /// Left edge of a column, in canvas pixels.
    pub fn column_x(&self, column: usize, advance: i32) -> i32 {
        self.ox + column as i32 * advance
    }

    /// The ascender line of a row, in canvas pixels.
    pub fn row_top(&self, row: usize, line_height: i32) -> i32 {
        self.oy + row as i32 * line_height
    }
}

/// The size and placement shared by every frame of one animation.
pub fn layout(
    frames: &[TextFrame],
    metrics: Metrics,
    canvas: Option<(u32, u32)>,
) -> Result<Layout> {
    let mut cols = 0usize;
    let mut rows = 0usize;
    for frame in frames {
        rows = rows.max(frame.lines.len());
        for line in &frame.lines {
            cols = cols.max(line.len());
        }
    }
    if cols == 0 || rows == 0 {
        return Err(Error::invalid("frames are empty"));
    }

    let width = (cols * metrics.advance as usize).max(1) as u32;
    let height = (rows * metrics.line_height as usize).max(1) as u32;

    let (out_width, out_height, ox, oy) = match canvas {
        Some((cw, ch)) => {
            if cw < width || ch < height {
                return Err(Error::invalid(format!(
                    "canvas {cw}x{ch} is smaller than the {width}x{height} text grid"
                )));
            }
            (
                cw,
                ch,
                ((cw - width) / 2) as i32,
                ((ch - height) / 2) as i32,
            )
        }
        None => (width, height, 0, 0),
    };

    Ok(Layout {
        width: out_width,
        height: out_height,
        cols,
        rows,
        ox,
        oy,
    })
}

/// Draws cells with one face at one size, caching each glyph mask.
pub struct TextRenderer<'a> {
    face: &'a Face,
    metrics: Metrics,
    masks: HashMap<u16, Option<Mask>>,
}

impl<'a> TextRenderer<'a> {
    pub fn new(face: &'a Face, size_px: u16) -> Result<TextRenderer<'a>> {
        let metrics = face.metrics(size_px)?;
        Ok(TextRenderer {
            face,
            metrics,
            masks: HashMap::new(),
        })
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    pub fn face(&self) -> &Face {
        self.face
    }

    /// Draw one frame on a fresh canvas of the layout size.
    pub fn draw(
        &mut self,
        frame: &TextFrame,
        layout: &Layout,
        background: [u8; 3],
    ) -> Result<Frame> {
        let mut image = Frame::new(layout.width, layout.height)?;
        image.fill(background);
        let advance = self.metrics.advance;
        let line_height = self.metrics.line_height;
        for (row, line) in frame.lines.iter().enumerate() {
            if row >= layout.rows {
                break;
            }
            let baseline = layout.row_top(row, line_height) + self.metrics.ascent;
            for (column, cell) in line.iter().enumerate() {
                if cell.ch == ' ' {
                    continue;
                }
                let pen_x = layout.column_x(column, advance);
                if let Some(mask) = self.mask_for(cell.ch)? {
                    blit(&mut image, mask, pen_x, baseline, cell.fg);
                }
            }
        }
        Ok(image)
    }

    fn mask_for(&mut self, ch: char) -> Result<Option<&Mask>> {
        let gid = self.face.glyph_index(ch).unwrap_or(0);
        if !self.masks.contains_key(&gid) {
            let mask = outline::flatten(self.face, gid, self.metrics.size_px)
                .map(|contours| raster::rasterize(&contours))?;
            self.masks.insert(gid, mask);
        }
        Ok(self.masks.get(&gid).and_then(|mask| mask.as_ref()))
    }
}

fn blit(frame: &mut Frame, mask: &Mask, pen_x: i32, baseline: i32, fg: [u8; 3]) {
    let origin_x = pen_x + mask.left;
    let origin_y = baseline + mask.top;
    let width = mask.width as usize;
    for row in 0..mask.height as i32 {
        let y = origin_y + row;
        if y < 0 || y >= frame.height() as i32 {
            continue;
        }
        for column in 0..mask.width as i32 {
            let coverage = mask.alpha[row as usize * width + column as usize];
            if coverage == 0 {
                continue;
            }
            let x = origin_x + column;
            if x < 0 || x >= frame.width() as i32 {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let dst = frame.pixel(x, y);
            frame.set_pixel(x, y, mix(dst, fg, coverage));
        }
    }
}

fn mix(dst: [u8; 3], fg: [u8; 3], coverage: u8) -> [u8; 3] {
    let a = coverage as u32;
    let keep = 255 - a;
    let mut out = [0u8; 3];
    for i in 0..3 {
        out[i] = ((dst[i] as u32 * keep + fg[i] as u32 * a + 127) / 255) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sgr;

    fn frame_of(lines: &[&str], delay_ms: u32) -> TextFrame {
        TextFrame {
            lines: lines
                .iter()
                .map(|line| sgr::parse_line(line, [230, 237, 246]))
                .collect(),
            delay_ms,
        }
    }

    fn metrics() -> Metrics {
        Face::embedded().metrics(28).unwrap()
    }

    #[test]
    fn a_grid_is_advance_wide_and_line_height_tall() {
        let frames = vec![frame_of(&["0123456789", "abcdefghij", "ABCDEFGHIJ"], 100)];
        let layout = layout(&frames, metrics(), None).unwrap();
        assert_eq!((layout.width, layout.height), (170, 99));
        assert_eq!((layout.cols, layout.rows), (10, 3));
    }

    #[test]
    fn a_fixed_canvas_centres_the_grid() {
        let frames = vec![frame_of(&["0123456789", "abcdefghij", "ABCDEFGHIJ"], 100)];
        let layout = layout(&frames, metrics(), Some((480, 270))).unwrap();
        assert_eq!((layout.width, layout.height), (480, 270));
        assert_eq!(layout.column_x(0, 17), 155);
        assert_eq!(layout.row_top(0, 33) + metrics().ascent, 85 + 26);
    }

    #[test]
    fn a_canvas_smaller_than_the_grid_is_rejected() {
        let frames = vec![frame_of(&["0123456789"], 100)];
        let err = layout(&frames, metrics(), Some((160, 270))).unwrap_err();
        assert!(err.to_string().contains("smaller"));
    }

    #[test]
    fn an_empty_frame_set_is_rejected() {
        let frames = vec![frame_of(&[], 100)];
        assert!(layout(&frames, metrics(), None).is_err());
    }

    #[test]
    fn drawing_fills_the_background_and_puts_ink_on_glyphs() {
        let frames = vec![frame_of(&["M"], 100)];
        let layout = layout(&frames, metrics(), None).unwrap();
        let mut renderer = TextRenderer::new(Face::embedded(), 28).unwrap();
        let image = renderer.draw(&frames[0], &layout, [10, 13, 18]).unwrap();
        assert_eq!(image.size(), (17, 33));
        assert_eq!(image.pixel(0, 0), [10, 13, 18]);
        let lit = image
            .data()
            .chunks(3)
            .filter(|pixel| *pixel != [10, 13, 18])
            .count();
        assert!(lit > 20, "expected glyph ink, found {lit} pixels");
    }

    #[test]
    fn spaces_leave_the_canvas_untouched() {
        let frames = vec![frame_of(&["   "], 100)];
        let layout = layout(&frames, metrics(), None).unwrap();
        let mut renderer = TextRenderer::new(Face::embedded(), 28).unwrap();
        let image = renderer.draw(&frames[0], &layout, [10, 13, 18]).unwrap();
        assert!(image.data().chunks(3).all(|pixel| pixel == [10, 13, 18]));
    }

    #[test]
    fn drawing_is_deterministic() {
        let frames = vec![frame_of(&["M M"], 100)];
        let layout = layout(&frames, metrics(), None).unwrap();
        let mut renderer = TextRenderer::new(Face::embedded(), 28).unwrap();
        let first = renderer.draw(&frames[0], &layout, [0, 0, 0]).unwrap();
        let second = renderer.draw(&frames[0], &layout, [0, 0, 0]).unwrap();
        assert_eq!(first.size(), second.size());
        assert_eq!(first.data(), second.data());
    }

    #[test]
    fn glyphs_clip_at_the_canvas_edge() {
        let mut image = Frame::new(4, 4).unwrap();
        image.fill([0, 0, 0]);
        let mask = Mask {
            left: -2,
            top: 0,
            width: 4,
            height: 4,
            alpha: vec![255; 16],
        };
        blit(&mut image, &mask, 0, 0, [255, 255, 255]);
        assert_eq!(image.pixel(0, 0), [255, 255, 255]);
        assert_eq!(image.pixel(1, 0), [255, 255, 255]);
        assert_eq!(image.pixel(2, 0), [0, 0, 0]);
        assert_eq!(image.pixel(3, 0), [0, 0, 0]);
    }
}
