//! The 10 ms grid, shared by the writer and the time crate.

/// The smallest delay a GIF can carry, in milliseconds.
pub const MIN_DELAY_MS: u32 = 20;

/// The GIF delay unit, in milliseconds.
pub const DELAY_UNIT_MS: u32 = 10;

/// Snap one delay onto the 10 ms grid, never below 20 ms.
pub fn snap_delay(ms: u32) -> u32 {
    let units = (ms + DELAY_UNIT_MS / 2) / DELAY_UNIT_MS;
    (units * DELAY_UNIT_MS).max(MIN_DELAY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_the_grid() {
        assert_eq!(snap_delay(0), 20);
        assert_eq!(snap_delay(14), 20);
        assert_eq!(snap_delay(16), 20);
        assert_eq!(snap_delay(25), 30);
        assert_eq!(snap_delay(100), 100);
        assert_eq!(snap_delay(104), 100);
    }
}
