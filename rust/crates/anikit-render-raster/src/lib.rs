//! Image decoding, EXIF rotation, fit, Ken Burns, crossfade and adjustments.
//!
//! This crate turns image files into frames, and reads a GIF or APNG back into
//! frames and a timing manifest (part 13). It does not choose a palette.

mod decode;
mod extract;
mod fit;
mod slideshow;

pub use extract::{ExtractInfo, Manifest, SourceFormat, extract, frame_name, read_manifest};
pub use fit::Fit;
pub use slideshow::{Mode, Options, Rendered, write_slideshow, write_timed};
