//! The bounds every route shares: the largest canvas and frame count.

use crate::{Error, Result};

/// The largest canvas side any route writes or reads, in pixels.
pub const MAX_CANVAS: u32 = 8192;

/// The largest frame count any route or frame plan may produce.
pub const MAX_FRAMES: u32 = 100_000;

/// Refuse a canvas larger than the shared limit.
pub fn check_canvas(width: u32, height: u32) -> Result<()> {
    if width > MAX_CANVAS || height > MAX_CANVAS {
        return Err(Error::invalid(format!(
            "canvas {width} x {height} is larger than the {MAX_CANVAS} px limit"
        )));
    }
    Ok(())
}

/// Refuse a frame count above the shared limit.
pub fn check_frames(count: u64) -> Result<()> {
    if count > MAX_FRAMES as u64 {
        return Err(Error::invalid(format!(
            "frame count {count} is larger than the {MAX_FRAMES} limit"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limits_are_inclusive() {
        assert!(check_canvas(8192, 8192).is_ok());
        assert!(check_canvas(8193, 100).is_err());
        assert!(check_canvas(100, 8193).is_err());
        assert!(check_frames(100_000).is_ok());
        assert!(check_frames(100_001).is_err());
        assert!(check_frames(4_294_967_295).is_err());
    }
}
