//! Image decoding, EXIF rotation, fit, Ken Burns, crossfade and adjustments.
//!
//! This crate turns image files into frames. It does not choose a palette.

mod decode;
mod fit;
mod slideshow;

pub use fit::Fit;
pub use slideshow::{Mode, Options, Rendered, write_slideshow};
