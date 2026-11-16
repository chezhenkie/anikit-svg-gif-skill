//! Presentation times, the 10 ms grid, cumulative rounding and the frame plan.
//!
//! Time only. This crate holds no pixels and no property values.

use anikit_core::{Error, MIN_DELAY_MS, Result, check_frames, snap_total};

/// The highest frame rate a GIF plays reliably.
pub const MAX_FPS: f64 = 50.0;

/// How a plan repeats when it is played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    #[default]
    Once,
    Cycle,
    PingPong,
}

/// Delays in milliseconds, on the grid, plus the presentation time of each frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FramePlan {
    delays: Vec<u32>,
    starts: Vec<u32>,
    total_ms: u32,
}

impl FramePlan {
    /// A plan from explicit delays, snapped on the running total.
    pub fn from_delays(delays: &[u32]) -> FramePlan {
        FramePlan::cumulative(delays)
    }

    /// A plan of `count` frames at `fps`, each start snapped to the grid.
    ///
    /// The exact frame times `i * 1000 / fps` are snapped and differenced, so
    /// a 12 fps plan alternates 80 and 90 ms and ends on its exact total.
    pub fn uniform(count: usize, fps: f64) -> Result<FramePlan> {
        if !fps.is_finite() || fps <= 0.0 {
            return Err(Error::time("fps must be greater than 0"));
        }
        if fps > MAX_FPS {
            return Err(Error::time(format!("fps must be {MAX_FPS} or less")));
        }
        if count == 0 {
            return Err(Error::time("frame count must be greater than 0"));
        }
        check_frames(count as u64)?;
        let mut delays = Vec::with_capacity(count);
        let mut starts = Vec::with_capacity(count);
        let mut previous = 0u32;
        for index in 0..=count {
            let exact_ms = index as f64 * 1000.0 / fps;
            let point = snap_total(exact_ms.round() as u32);
            if index > 0 {
                starts.push(previous);
                delays.push(point.saturating_sub(previous).max(MIN_DELAY_MS));
            }
            previous = point;
        }
        let total_ms = starts
            .last()
            .copied()
            .unwrap_or(0)
            .saturating_add(delays.last().copied().unwrap_or(0));
        Ok(FramePlan {
            delays,
            starts,
            total_ms,
        })
    }

    /// Delays from requested durations, rounded on cumulative time.
    ///
    /// Rounding the running total and differencing keeps the error from
    /// accumulating around a loop, which is what the frozen Python does.
    pub fn cumulative(durations_ms: &[u32]) -> FramePlan {
        let mut delays = Vec::with_capacity(durations_ms.len());
        let mut starts = Vec::with_capacity(durations_ms.len());
        let mut elapsed = 0u32;
        let mut emitted = 0u32;
        for ms in durations_ms {
            elapsed = elapsed.saturating_add(*ms);
            let target = snap_total(elapsed);
            let delay = target.saturating_sub(emitted).max(MIN_DELAY_MS);
            starts.push(emitted);
            delays.push(delay);
            emitted = emitted.saturating_add(delay);
        }
        FramePlan {
            delays,
            starts,
            total_ms: emitted,
        }
    }

    pub fn len(&self) -> usize {
        self.delays.len()
    }

    pub fn is_empty(&self) -> bool {
        self.delays.is_empty()
    }

    pub fn delays(&self) -> &[u32] {
        &self.delays
    }

    pub fn starts(&self) -> &[u32] {
        &self.starts
    }

    pub fn total_ms(&self) -> u32 {
        self.total_ms
    }

    /// The presentation time of one frame in microseconds.
    pub fn presentation_us(&self, index: usize) -> Option<u64> {
        self.starts.get(index).map(|ms| *ms as u64 * 1000)
    }

    /// Drop the final frame when it repeats the first, which a cycle does not need.
    pub fn without_last(&self) -> FramePlan {
        if self.delays.len() <= 1 {
            return self.clone();
        }
        FramePlan::from_delays(&self.delays[..self.delays.len() - 1])
    }

    /// Presentation times for a repeated play, in microseconds.
    pub fn play(&self, mode: LoopMode, repeats: usize) -> Vec<u64> {
        let repeats = repeats.max(1);
        let mut order: Vec<usize> = Vec::new();
        for _ in 0..repeats {
            match mode {
                LoopMode::Once => order.extend(0..self.len()),
                LoopMode::Cycle => order.extend(0..self.len()),
                LoopMode::PingPong => {
                    order.extend(0..self.len());
                    if self.len() > 2 {
                        order.extend((1..self.len() - 1).rev());
                    }
                }
            }
            if mode == LoopMode::Once {
                break;
            }
        }
        order
            .into_iter()
            .filter_map(|i| self.presentation_us(i))
            .collect()
    }
}

/// Snap a delay onto the grid, at least one unit and never below the minimum.
pub fn snap_ms(ms: u32) -> u32 {
    snap_total(ms).max(MIN_DELAY_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_rounding_does_not_drift() {
        let plan = FramePlan::cumulative(&[33, 33, 33]);
        assert_eq!(plan.delays(), &[30, 40, 30]);
        assert_eq!(plan.starts(), &[0, 30, 70]);
        assert_eq!(plan.total_ms(), 100);
        let same = FramePlan::from_delays(&[33, 33, 33]);
        assert_eq!(same.delays(), plan.delays());
        assert_eq!(same.total_ms(), plan.total_ms());
    }

    #[test]
    fn short_delays_are_raised_to_the_minimum() {
        let plan = FramePlan::cumulative(&[1, 1, 1]);
        assert_eq!(plan.delays(), &[20, 20, 20]);
    }

    #[test]
    fn uniform_uses_the_grid() {
        let plan = FramePlan::uniform(3, 15.0).unwrap();
        assert_eq!(plan.delays(), &[70, 60, 70]);
        assert_eq!(plan.presentation_us(1), Some(70_000));
        assert_eq!(plan.total_ms(), 200);
    }

    #[test]
    fn the_frame_count_is_capped() {
        assert!(FramePlan::uniform(250, 25.0).is_ok());
        assert!(FramePlan::uniform(100_000, 50.0).is_ok());
        let err = FramePlan::uniform(100_001, 50.0).unwrap_err();
        assert!(err.to_string().contains("100001"), "{err}");
    }

    #[test]
    fn uniform_twelve_fps_totals_one_second() {
        let plan = FramePlan::uniform(12, 12.0).unwrap();
        assert_eq!(plan.total_ms(), 1000);
        assert_eq!(
            plan.delays(),
            &[80, 90, 80, 80, 90, 80, 80, 90, 80, 80, 90, 80]
        );
        assert_eq!(plan.presentation_us(11), Some(920_000));
    }

    #[test]
    fn frame_rate_limits_are_enforced() {
        assert!(FramePlan::uniform(2, 0.0).is_err());
        assert!(FramePlan::uniform(2, 51.0).is_err());
        assert!(FramePlan::uniform(0, 15.0).is_err());
        assert!(FramePlan::uniform(2, 50.0).is_ok());
    }

    #[test]
    fn a_total_at_the_top_of_the_range_does_not_wrap() {
        let plan = FramePlan::cumulative(&[u32::MAX, u32::MAX]);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan.delays()[0], 4_294_967_290);
        assert_eq!(plan.total_ms(), u32::MAX);
    }

    #[test]
    fn pingpong_does_not_repeat_the_endpoints() {
        let plan = FramePlan::from_delays(&[100, 100, 100]);
        let times = plan.play(LoopMode::PingPong, 1);
        assert_eq!(times, vec![0, 100_000, 200_000, 100_000]);
    }

    #[test]
    fn once_plays_a_single_pass() {
        let plan = FramePlan::from_delays(&[100, 100]);
        assert_eq!(plan.play(LoopMode::Once, 5).len(), 2);
    }

    #[test]
    fn without_last_drops_the_duplicate_seam_frame() {
        let plan = FramePlan::from_delays(&[100, 100, 100]);
        let trimmed = plan.without_last();
        assert_eq!(trimmed.len(), 2);
        assert_eq!(trimmed.total_ms(), 200);
        let single = FramePlan::from_delays(&[100]);
        assert_eq!(single.without_last().len(), 1);
    }
}
