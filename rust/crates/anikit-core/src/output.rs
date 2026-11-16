//! The output format and the summary every writer reports.

use std::path::Path;

use crate::{Error, Result};

/// A format the binary can write. GIF and APNG come from frames; SVG is only
/// written by the tween route, which owns the timeline (part 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Gif,
    Apng,
    Svg,
}

impl OutputFormat {
    /// Choose the format from the output extension. There is no default: a
    /// path without one of these endings is refused before any work starts.
    pub fn from_path(path: &Path) -> Result<OutputFormat> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        match extension.as_deref() {
            Some("gif") => Ok(OutputFormat::Gif),
            Some("png") | Some("apng") => Ok(OutputFormat::Apng),
            Some("svg") => Ok(OutputFormat::Svg),
            _ => Err(Error::invalid(
                "output must end in .gif, .png, .apng or .svg",
            )),
        }
    }

    /// True when the format is built from frames.
    pub fn from_frames(self) -> bool {
        self != OutputFormat::Svg
    }

    /// Refuse an SVG target on a route that only produces frames.
    pub fn require_frames(self) -> Result<()> {
        if self.from_frames() {
            Ok(())
        } else {
            Err(Error::invalid(
                "SVG output is only written by the tween route",
            ))
        }
    }
}

/// What a writer produced, for the caller's summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSummary {
    pub frames: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
}
