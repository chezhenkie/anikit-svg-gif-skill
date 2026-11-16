//! Frames, palette, GIF and APNG writers, and readback verification.
//!
//! This crate owns bytes and files only. It knows nothing about fonts, SVG,
//! image decoding or animation; those live in the renderer crates.
//!
//! The rules it enforces are the ones in docs/specsheet/01-behaviour-baseline.md,
//! docs/specsheet/03-frame-flow.md and docs/specsheet/12-output-formats.md: one
//! palette per GIF loop, delays on the 10 ms grid, identical frames merged, a
//! temp file verified before it replaces the target.

mod apng;
mod encode;
mod error;
mod frame;
mod inspect;
mod output;
mod palette;
mod quantize;
mod stream;
mod time;
mod writer;

pub use apng::{ApngStream, stream_apng};
pub use encode::GifEncoder;
pub use error::{Error, Result};
pub use frame::Frame;
pub use inspect::{OutputInfo, inspect, inspect_apng, inspect_apng_file, inspect_file};
pub use output::{OutputFormat, OutputSummary};
pub use palette::Palette;
pub use quantize::{Dither, Quantizer};
pub use stream::{FrameSink, GifStream, OutputStream, StreamOptions, stream_output};
pub use time::{DELAY_UNIT_MS, MIN_DELAY_MS, snap_delay, snap_total};
pub use writer::temp_path;
