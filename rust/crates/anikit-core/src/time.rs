//! The 10 ms grid, shared by the writer and the time crate.

/// The smallest delay a GIF can carry, in milliseconds.
pub const MIN_DELAY_MS: u32 = 20;

/// The GIF delay unit, in milliseconds.
pub const DELAY_UNIT_MS: u32 = 10;

/// Snap a running total onto the grid, without the minimum applied.
///
/// The sum saturates, so a total near `u32::MAX` snaps instead of wrapping.
pub fn snap_total(ms: u32) -> u32 {
    ((ms.saturating_add(DELAY_UNIT_MS / 2)) / DELAY_UNIT_MS) * DELAY_UNIT_MS
}

/// Snap one delay onto the 10 ms grid, never below 20 ms.
pub fn snap_delay(ms: u32) -> u32 {
    snap_total(ms).max(MIN_DELAY_MS)
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

    #[test]
    fn totals_snap_without_a_minimum() {
        assert_eq!(snap_total(0), 0);
        assert_eq!(snap_total(4), 0);
        assert_eq!(snap_total(6), 10);
        assert_eq!(snap_total(33), 30);
        assert_eq!(snap_total(36), 40);
    }

    #[test]
    fn a_total_at_the_top_of_the_range_snaps() {
        assert_eq!(snap_total(u32::MAX), 4_294_967_290);
        assert_eq!(snap_delay(u32::MAX), 4_294_967_290);
    }
}
