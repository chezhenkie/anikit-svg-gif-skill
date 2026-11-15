//! Glyph grids, ANSI SGR parsing, glyph loading and metrics.
//!
//! This crate draws text into frames. It does not quantise or write files.
//! One face ships with it: DejaVu Sans Mono regular, in `assets/`, with its
//! licence beside it. `--font` and `ANIM_GIF_FONT` may replace it.

mod font;
mod input;
mod outline;
mod raster;
mod render;
mod sgr;

pub use font::{EMBEDDED_TTF, Face, Metrics, Point};
pub use input::{load_frames, parse_frames};
pub use raster::Mask;
pub use render::{Layout, TextRenderer, layout};
pub use sgr::{parse_line, xterm_rgb};

/// One character cell and the colour its glyph is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: [u8; 3],
}

/// One text frame: the parsed lines and the delay it is held for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextFrame {
    pub lines: Vec<Vec<Cell>>,
    pub delay_ms: u32,
}
