//! Frames, palette, GIF writer and readback verification.
//!
//! This crate owns bytes and files only. It knows nothing about fonts, SVG,
//! image decoding or animation; those live in the renderer crates.
//!
//! The rules it enforces are the ones in docs/specsheet/01-behaviour-baseline.md
//! and docs/specsheet/03-frame-flow.md: one palette per loop, delays on the
//! 10 ms grid, identical frames merged, a temp file verified before it replaces
//! the target.

mod encode;
mod error;
mod frame;
mod inspect;
mod merge;
mod palette;
mod quantize;
mod time;
mod writer;

pub use encode::{GifEncoder, GifSummary};
pub use error::{Error, Result};
pub use frame::Frame;
pub use inspect::{GifInfo, inspect, inspect_file};
pub use merge::{Run, merge_runs};
pub use palette::Palette;
pub use quantize::{Dither, Quantizer};
pub use time::{DELAY_UNIT_MS, MIN_DELAY_MS, snap_delay};
pub use writer::{WriteOptions, write_gif};
