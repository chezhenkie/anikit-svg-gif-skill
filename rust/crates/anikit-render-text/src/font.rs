//! TrueType tables for the text route: metrics, cmap lookup and glyph outlines.
//!
//! Only the tables a monospace grid needs are read: `head`, `hhea`, `maxp`,
//! `cmap`, `hmtx`, `loca` and `glyf`. Hinting instructions are ignored, so
//! outlines are scaled from font units and antialiased by the rasteriser.

use std::path::Path;
use std::sync::OnceLock;

use anikit_core::{Error, Result};

/// The bundled face, unmodified from the DejaVu 2.37 release.
pub const EMBEDDED_TTF: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");

const TRUE_TYPE: u32 = 0x0001_0000;
const TRUE_TAG: u32 = 0x7472_7565;
const MAX_COMPOSITE_DEPTH: u8 = 8;

/// One outline point in font units, y up. Off-curve points are quadratic controls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub on: bool,
}

/// Pixel metrics for one size, rounded the way FreeType reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub size_px: u16,
    pub ascent: i32,
    pub descent: i32,
    pub line_height: i32,
    pub advance: i32,
}

/// A parsed TrueType face.
pub struct Face {
    data: Vec<u8>,
    upem: u16,
    ascender: i16,
    descender: i16,
    num_glyphs: u16,
    num_h_metrics: u16,
    loca_format: i16,
    cmap_at: usize,
    cmap_format: u16,
    glyf_at: usize,
    loca_at: usize,
    hmtx_at: usize,
}

impl Face {
    /// Parse a face from owned bytes.
    pub fn from_bytes(data: Vec<u8>) -> Result<Face> {
        if data.len() < 12 {
            return Err(Error::invalid("font is too short to be a TrueType file"));
        }
        let version = be_u32(&data, 0).ok_or_else(|| Error::invalid("font header is cut off"))?;
        if version == 0x4F54_544F {
            return Err(Error::invalid(
                "font uses CFF outlines; a TrueType glyf face is required",
            ));
        }
        if version != TRUE_TYPE && version != TRUE_TAG {
            return Err(Error::invalid("font is not a TrueType file"));
        }

        let head = table(&data, *b"head")?;
        let hhea = table(&data, *b"hhea")?;
        let maxp = table(&data, *b"maxp")?;
        let cmap = table(&data, *b"cmap")?;
        let glyf = table(&data, *b"glyf")?;
        let loca = table(&data, *b"loca")?;
        let hmtx = table(&data, *b"hmtx")?;

        let upem = be_u16(&data, head.0 + 18)
            .filter(|u| *u >= 16)
            .ok_or_else(|| Error::invalid("font has no usable units per em"))?;
        let loca_format = be_i16(&data, head.0 + 50)
            .ok_or_else(|| Error::invalid("font head table is cut off"))?;
        let ascender = be_i16(&data, hhea.0 + 4)
            .ok_or_else(|| Error::invalid("font hhea table is cut off"))?;
        let descender = be_i16(&data, hhea.0 + 6)
            .ok_or_else(|| Error::invalid("font hhea table is cut off"))?;
        let num_h_metrics = be_u16(&data, hhea.0 + 34)
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::invalid("font has no horizontal metrics"))?;
        let num_glyphs = be_u16(&data, maxp.0 + 4)
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::invalid("font has no glyphs"))?;
        if ascender <= descender {
            return Err(Error::invalid("font ascender is not above its descender"));
        }
        let entries = num_glyphs as usize + 1;
        let loca_bytes = entries * if loca_format == 0 { 2 } else { 4 };
        if loca.1 < loca_bytes {
            return Err(Error::invalid(
                "font loca table is short for its glyph count",
            ));
        }

        let (cmap_at, cmap_format) = select_cmap(&data, cmap.0, cmap.1)?;

        Ok(Face {
            data,
            upem,
            ascender,
            descender,
            num_glyphs,
            num_h_metrics,
            loca_format,
            cmap_at,
            cmap_format,
            glyf_at: glyf.0,
            loca_at: loca.0,
            hmtx_at: hmtx.0,
        })
    }

    /// Load a face from disk.
    pub fn from_path(path: &Path) -> Result<Face> {
        let data = std::fs::read(path)
            .map_err(|err| Error::io(format!("cannot read font {}: {err}", path.display())))?;
        Face::from_bytes(data)
    }

    /// The face bundled with the crate.
    pub fn embedded() -> &'static Face {
        static FACE: OnceLock<Face> = OnceLock::new();
        FACE.get_or_init(|| Face::from_bytes(EMBEDDED_TTF.to_vec()).expect("bundled font is valid"))
    }

    /// Pixel metrics at a size, rounded to whole pixels.
    pub fn metrics(&self, size_px: u16) -> Result<Metrics> {
        if size_px == 0 {
            return Err(Error::invalid("font size must be greater than 0"));
        }
        if size_px > 1024 {
            return Err(Error::invalid("font size must be 1024 or less"));
        }
        let scale = size_px as f32 / self.upem as f32;
        let ascent = (self.ascender as f32 * scale).round() as i32;
        let descent = (-self.descender as f32 * scale).round() as i32;
        let advance = match self
            .glyph_index('M')
            .and_then(|gid| self.advance_units(gid))
        {
            Some(units) => (units as f32 * scale).round() as i32,
            None => (self.upem as f32 * 0.6 * scale).round() as i32,
        };
        Ok(Metrics {
            size_px,
            ascent,
            descent,
            line_height: (ascent + descent).max(1),
            advance: advance.max(1),
        })
    }

    /// The design size of one em, in font units.
    pub fn units_per_em(&self) -> u16 {
        self.upem
    }

    /// The advance width in font units.
    pub fn advance_units(&self, gid: u16) -> Option<u16> {
        let index = gid.min(self.num_h_metrics - 1);
        be_u16(&self.data, self.hmtx_at + index as usize * 4)
    }

    /// True when the face advances every glyph it is asked about by one width.
    pub fn is_monospace(&self) -> bool {
        let reference = self.glyph_index('M').and_then(|g| self.advance_units(g));
        match reference {
            Some(width) => ['i', 'w', '1', '.'].iter().all(|ch| {
                self.glyph_index(*ch)
                    .and_then(|g| self.advance_units(g))
                    .map(|w| w == width)
                    .unwrap_or(true)
            }),
            None => false,
        }
    }

    /// The glyph number for a character, or None when the face has no glyph.
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        match self.cmap_format {
            4 => cmap4_lookup(&self.data, self.cmap_at, ch),
            12 => cmap12_lookup(&self.data, self.cmap_at, ch),
            _ => None,
        }
        .filter(|gid| *gid < self.num_glyphs)
    }

    /// Outlines for one glyph in font units, composites resolved.
    pub fn glyph_points(&self, gid: u16) -> Result<Vec<Vec<Point>>> {
        let mut out = Vec::new();
        self.read_glyph(gid, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 0, &mut out)?;
        Ok(out)
    }

    fn read_glyph(
        &self,
        gid: u16,
        matrix: [f32; 6],
        depth: u8,
        out: &mut Vec<Vec<Point>>,
    ) -> Result<()> {
        if gid >= self.num_glyphs {
            return Err(Error::invalid(format!("glyph {gid} is out of range")));
        }
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(Error::invalid("composite glyph nests too deeply"));
        }
        let (start, end) = self.glyph_range(gid)?;
        if start == end {
            return Ok(());
        }
        let base = self.glyf_at + start;
        let contours =
            be_i16(&self.data, base).ok_or_else(|| Error::invalid("glyph header is cut off"))?;
        match contours {
            // A glyph with no contours is empty even though its loca range is
            // non-empty; there is no contour table to read.
            0 => {}
            c if c > 0 => {
                let mut points = self.simple_points(gid, base, c as usize)?;
                apply_matrix(&mut points, matrix);
                out.extend(points);
            }
            _ => self.composite_points(base, matrix, depth, out)?,
        }
        Ok(())
    }

    fn glyph_range(&self, gid: u16) -> Result<(usize, usize)> {
        let at = self.loca_at + gid as usize * if self.loca_format == 0 { 2 } else { 4 };
        let (start, end) = if self.loca_format == 0 {
            let start = be_u16(&self.data, at).ok_or_else(|| Error::invalid("loca is cut off"))?;
            let end =
                be_u16(&self.data, at + 2).ok_or_else(|| Error::invalid("loca is cut off"))?;
            (start as usize * 2, end as usize * 2)
        } else {
            let start = be_u32(&self.data, at).ok_or_else(|| Error::invalid("loca is cut off"))?;
            let end =
                be_u32(&self.data, at + 4).ok_or_else(|| Error::invalid("loca is cut off"))?;
            (start as usize, end as usize)
        };
        if end < start || end > self.data.len().saturating_sub(self.glyf_at) {
            return Err(Error::invalid(format!("glyph {gid} has a bad location")));
        }
        Ok((start, end))
    }

    fn simple_points(&self, gid: u16, base: usize, contours: usize) -> Result<Vec<Vec<Point>>> {
        let data = &self.data;
        let ends_at = base + 10;
        let last = be_u16(data, ends_at + (contours - 1) * 2)
            .ok_or_else(|| Error::invalid("glyph contour table is cut off"))?
            as usize;
        let count = last + 1;
        let instr_at = ends_at + contours * 2;
        let instr_len = be_u16(data, instr_at)
            .ok_or_else(|| Error::invalid("glyph instructions are cut off"))?
            as usize;

        let mut ends = Vec::with_capacity(contours);
        for i in 0..contours {
            ends.push(
                be_u16(data, ends_at + i * 2)
                    .ok_or_else(|| Error::invalid("glyph contour table is cut off"))?
                    as usize,
            );
        }

        let mut at = instr_at + 2 + instr_len;
        let mut flags = Vec::with_capacity(count);
        while flags.len() < count {
            let flag = *data
                .get(at)
                .ok_or_else(|| Error::invalid("glyph flags are cut off"))?;
            at += 1;
            flags.push(flag);
            if flag & 8 != 0 {
                let repeat = *data
                    .get(at)
                    .ok_or_else(|| Error::invalid("glyph flag repeat is cut off"))?;
                at += 1;
                for _ in 0..repeat {
                    if flags.len() >= count {
                        break;
                    }
                    flags.push(flag);
                }
            }
        }

        let mut xs = vec![0i32; count];
        let mut value = 0i32;
        for (i, flag) in flags.iter().enumerate() {
            let delta = if flag & 2 != 0 {
                let byte = *data
                    .get(at)
                    .ok_or_else(|| Error::invalid("glyph x coordinates are cut off"))?;
                at += 1;
                if flag & 16 != 0 {
                    byte as i32
                } else {
                    -(byte as i32)
                }
            } else if flag & 16 != 0 {
                0
            } else {
                let word = be_i16(data, at)
                    .ok_or_else(|| Error::invalid("glyph x coordinates are cut off"))?;
                at += 2;
                word as i32
            };
            value = add_point(value, delta, gid)?;
            xs[i] = value;
        }

        let mut ys = vec![0i32; count];
        value = 0;
        for (i, flag) in flags.iter().enumerate() {
            let delta = if flag & 4 != 0 {
                let byte = *data
                    .get(at)
                    .ok_or_else(|| Error::invalid("glyph y coordinates are cut off"))?;
                at += 1;
                if flag & 32 != 0 {
                    byte as i32
                } else {
                    -(byte as i32)
                }
            } else if flag & 32 != 0 {
                0
            } else {
                let word = be_i16(data, at)
                    .ok_or_else(|| Error::invalid("glyph y coordinates are cut off"))?;
                at += 2;
                word as i32
            };
            value = add_point(value, delta, gid)?;
            ys[i] = value;
        }

        let mut out = Vec::with_capacity(contours);
        let mut start = 0usize;
        for end in ends {
            if end < start || end >= count {
                return Err(Error::invalid("glyph contour ends are out of order"));
            }
            let mut contour = Vec::with_capacity(end - start + 1);
            for i in start..=end {
                contour.push(Point {
                    x: xs[i] as f32,
                    y: ys[i] as f32,
                    on: flags[i] & 1 != 0,
                });
            }
            out.push(contour);
            start = end + 1;
        }
        Ok(out)
    }

    fn composite_points(
        &self,
        base: usize,
        matrix: [f32; 6],
        depth: u8,
        out: &mut Vec<Vec<Point>>,
    ) -> Result<()> {
        let mut at = base + 10;
        loop {
            let flags = be_u16(&self.data, at)
                .ok_or_else(|| Error::invalid("composite glyph is cut off"))?;
            let gid = be_u16(&self.data, at + 2)
                .ok_or_else(|| Error::invalid("composite glyph is cut off"))?;
            at += 4;

            let (dx, dy) = if flags & 1 != 0 {
                let dx = be_i16(&self.data, at)
                    .ok_or_else(|| Error::invalid("composite offsets are cut off"))?;
                let dy = be_i16(&self.data, at + 2)
                    .ok_or_else(|| Error::invalid("composite offsets are cut off"))?;
                at += 4;
                (dx as f32, dy as f32)
            } else {
                let dx = *self
                    .data
                    .get(at)
                    .ok_or_else(|| Error::invalid("composite offsets are cut off"))?
                    as i8;
                let dy = *self
                    .data
                    .get(at + 1)
                    .ok_or_else(|| Error::invalid("composite offsets are cut off"))?
                    as i8;
                at += 2;
                (dx as f32, dy as f32)
            };
            if flags & 2 == 0 {
                return Err(Error::invalid(
                    "composite glyph places components by point index, which is not supported",
                ));
            }

            let (a, b, c, d) = if flags & 0x8 != 0 {
                let s = f2dot14(&self.data, at)?;
                at += 2;
                (s, 0.0, 0.0, s)
            } else if flags & 0x40 != 0 {
                let a = f2dot14(&self.data, at)?;
                let d = f2dot14(&self.data, at + 2)?;
                at += 4;
                (a, 0.0, 0.0, d)
            } else if flags & 0x80 != 0 {
                let a = f2dot14(&self.data, at)?;
                let b = f2dot14(&self.data, at + 2)?;
                let c = f2dot14(&self.data, at + 4)?;
                let d = f2dot14(&self.data, at + 6)?;
                at += 8;
                (a, b, c, d)
            } else {
                (1.0, 0.0, 0.0, 1.0)
            };

            let local = [a, b, c, d, dx, dy];
            let combined = multiply(matrix, local);
            self.read_glyph(gid, combined, depth + 1, out)?;

            if flags & 0x20 == 0 {
                break;
            }
        }
        Ok(())
    }
}

fn select_cmap(data: &[u8], at: usize, len: usize) -> Result<(usize, u16)> {
    let count =
        be_u16(data, at + 2).ok_or_else(|| Error::invalid("font cmap is cut off"))? as usize;
    let mut best: Option<(usize, u16, i32)> = None;
    for i in 0..count {
        let record = at + 4 + i * 8;
        let platform = be_u16(data, record);
        let encoding = be_u16(data, record + 2);
        let offset = be_u32(data, record + 4);
        let (Some(platform), Some(encoding), Some(offset)) = (platform, encoding, offset) else {
            continue;
        };
        let sub = at + offset as usize;
        if sub >= at + len {
            continue;
        }
        let Some(format) = be_u16(data, sub) else {
            continue;
        };
        let score = match (platform, encoding, format) {
            (3, 10, 12) | (0, 4, 12) | (0, 6, 12) => 6,
            (3, 1, 4) | (0, 3, 4) => 5,
            (_, _, 12) => 3,
            (_, _, 4) => 2,
            _ => continue,
        };
        if best.map(|(_, _, s)| score > s).unwrap_or(true) {
            best = Some((sub, format, score));
        }
    }
    best.map(|(at, format, _)| (at, format))
        .ok_or_else(|| Error::invalid("font has no Unicode cmap"))
}

fn cmap4_lookup(data: &[u8], at: usize, ch: char) -> Option<u16> {
    let code = u32::from(ch);
    if code > 0xFFFF {
        return None;
    }
    let code = code as u16;
    let seg_count = be_u16(data, at + 6)? as usize / 2;
    let ends_at = at + 14;
    let starts_at = ends_at + seg_count * 2 + 2;
    let deltas_at = starts_at + seg_count * 2;
    let ranges_at = deltas_at + seg_count * 2;

    for i in 0..seg_count {
        let end = be_u16(data, ends_at + i * 2)?;
        if code > end {
            continue;
        }
        let start = be_u16(data, starts_at + i * 2)?;
        if code < start {
            return None;
        }
        let delta = be_i16(data, deltas_at + i * 2)?;
        let range = be_u16(data, ranges_at + i * 2)?;
        if range == 0 {
            return Some(code.wrapping_add(delta as u16));
        }
        let index = ranges_at + i * 2 + range as usize + (code - start) as usize * 2;
        let glyph = be_u16(data, index)?;
        if glyph == 0 {
            return None;
        }
        return Some(glyph.wrapping_add(delta as u16));
    }
    None
}

fn cmap12_lookup(data: &[u8], at: usize, ch: char) -> Option<u16> {
    let code = u32::from(ch);
    let groups = be_u32(data, at + 12)? as usize;
    let mut low = 0usize;
    let mut high = groups;
    while low < high {
        let mid = (low + high) / 2;
        let record = at + 16 + mid * 12;
        let start = be_u32(data, record)?;
        let end = be_u32(data, record + 4)?;
        if code < start {
            high = mid;
        } else if code > end {
            low = mid + 1;
        } else {
            let first = be_u32(data, record + 8)?;
            return u16::try_from(first + (code - start)).ok();
        }
    }
    None
}

/// One coordinate step, refused rather than wrapped when it leaves `i32`.
fn add_point(value: i32, delta: i32, gid: u16) -> Result<i32> {
    value
        .checked_add(delta)
        .ok_or_else(|| Error::invalid(format!("glyph {gid} coordinate overflows")))
}

fn apply_matrix(points: &mut [Vec<Point>], m: [f32; 6]) {
    for contour in points.iter_mut() {
        for point in contour.iter_mut() {
            let x = point.x;
            let y = point.y;
            point.x = m[0] * x + m[2] * y + m[4];
            point.y = m[1] * x + m[3] * y + m[5];
        }
    }
}

fn multiply(outer: [f32; 6], inner: [f32; 6]) -> [f32; 6] {
    [
        outer[0] * inner[0] + outer[2] * inner[1],
        outer[1] * inner[0] + outer[3] * inner[1],
        outer[0] * inner[2] + outer[2] * inner[3],
        outer[1] * inner[2] + outer[3] * inner[3],
        outer[0] * inner[4] + outer[2] * inner[5] + outer[4],
        outer[1] * inner[4] + outer[3] * inner[5] + outer[5],
    ]
}

fn f2dot14(data: &[u8], at: usize) -> Result<f32> {
    let raw = be_i16(data, at).ok_or_else(|| Error::invalid("composite scale is cut off"))?;
    Ok(raw as f32 / 16384.0)
}

fn table(data: &[u8], tag: [u8; 4]) -> Result<(usize, usize)> {
    let count = be_u16(data, 4).ok_or_else(|| Error::invalid("font header is cut off"))? as usize;
    let wanted = u32::from_be_bytes(tag);
    for i in 0..count {
        let record = 12 + i * 16;
        let Some(name) = be_u32(data, record) else {
            break;
        };
        if name == wanted {
            let offset = be_u32(data, record + 8)
                .ok_or_else(|| Error::invalid("font table directory is cut off"))?
                as usize;
            let length = be_u32(data, record + 12)
                .ok_or_else(|| Error::invalid("font table directory is cut off"))?
                as usize;
            if offset > data.len() || length > data.len() - offset {
                return Err(Error::invalid("font table lies outside the file"));
            }
            return Ok((offset, length));
        }
    }
    let name = String::from_utf8_lossy(&tag).into_owned();
    Err(Error::invalid(format!("font has no {name} table")))
}

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_i16(data: &[u8], at: usize) -> Option<i16> {
    be_u16(data, at).map(|v| v as i16)
}

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_metrics_match_the_reference() {
        let face = Face::embedded();
        let metrics = face.metrics(28).unwrap();
        assert_eq!(metrics.ascent, 26);
        assert_eq!(metrics.descent, 7);
        assert_eq!(metrics.line_height, 33);
        assert_eq!(metrics.advance, 17);
    }

    #[test]
    fn embedded_face_is_monospace() {
        assert!(Face::embedded().is_monospace());
    }

    #[test]
    fn glyph_lookup_covers_ascii_and_block_elements() {
        let face = Face::embedded();
        assert!(face.glyph_index('M').is_some());
        assert!(face.glyph_index(' ').is_some());
        assert!(face.glyph_index('\u{2588}').is_some());
        assert!(face.glyph_index('\u{2500}').is_some());
    }

    #[test]
    fn space_has_no_outline_and_m_has_one() {
        let face = Face::embedded();
        let space = face.glyph_points(face.glyph_index(' ').unwrap()).unwrap();
        assert!(space.is_empty());
        let m = face.glyph_points(face.glyph_index('M').unwrap()).unwrap();
        assert!(!m.is_empty());
        let points: usize = m.iter().map(|c| c.len()).sum();
        assert!(points > 8);
    }

    #[test]
    fn composite_glyphs_resolve() {
        let face = Face::embedded();
        let gid = face.glyph_index('\u{e9}').expect("e acute");
        let contours = face.glyph_points(gid).unwrap();
        assert!(contours.len() >= 2);
    }

    #[test]
    fn sizes_are_bounded() {
        assert!(Face::embedded().metrics(0).is_err());
        assert!(Face::embedded().metrics(1025).is_err());
    }

    #[test]
    fn broken_fonts_are_rejected() {
        assert!(Face::from_bytes(vec![0; 4]).is_err());
        assert!(Face::from_bytes(vec![0; 64]).is_err());
    }

    /// The embedded face with one glyph's contour count overwritten.
    fn with_contours(face: &Face, gid: u16, contours: i16) -> Face {
        let mut data = face.data.clone();
        let (start, _end) = face.glyph_range(gid).unwrap();
        let base = face.glyf_at + start;
        data[base..base + 2].copy_from_slice(&contours.to_be_bytes());
        Face::from_bytes(data).unwrap()
    }

    /// One loca entry, in the encoding the face uses.
    fn set_loca(face: &Face, data: &mut [u8], index: u16, value: u32) {
        if face.loca_format == 0 {
            let at = face.loca_at + index as usize * 2;
            data[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
        } else {
            let at = face.loca_at + index as usize * 4;
            data[at..at + 4].copy_from_slice(&value.to_be_bytes());
        }
    }

    #[test]
    fn a_coordinate_that_leaves_its_range_is_refused() {
        let err = add_point(i32::MAX, 1, 7).unwrap_err();
        assert!(err.to_string().contains("glyph 7"), "{err}");
        assert_eq!(add_point(-5, 5, 7).unwrap(), 0);
    }

    #[test]
    fn a_glyph_with_no_contours_has_no_outline() {
        let face = Face::embedded();
        let gid = face.glyph_index('M').unwrap();
        assert!(!face.glyph_points(gid).unwrap().is_empty());
        let empty = with_contours(face, gid, 0);
        assert!(empty.glyph_points(gid).unwrap().is_empty());
    }

    #[test]
    fn a_glyph_outside_its_loca_range_is_refused() {
        let face = Face::embedded();
        let gid = face.glyph_index('M').unwrap();
        let mut data = face.data.clone();
        set_loca(face, &mut data, gid, 0xFFFF);
        set_loca(face, &mut data, gid + 1, 0);
        let broken = Face::from_bytes(data).unwrap();
        let err = broken.glyph_points(gid).unwrap_err();
        assert!(err.to_string().contains("bad location"), "{err}");
    }
}
