//! The time map that applies loop mode and rate before a timeline is sampled.
//!
//! Time is integer microseconds. A rate converts to a float once, at the top
//! of the map, and the mapped time returns to integer microseconds before any
//! track is sampled.

use anikit_core::{Error, Result};

pub use anikit_time::LoopMode;

/// How a repeated or reversed play maps presentation time onto local time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timing {
    pub loop_mode: LoopMode,
    pub rate: f64,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            loop_mode: LoopMode::Once,
            rate: 1.0,
        }
    }
}

impl Timing {
    pub fn new(loop_mode: LoopMode, rate: f64) -> Timing {
        Timing { loop_mode, rate }
    }

    /// A rate must be a real number. Zero is legal and freezes the animation.
    pub fn validate(&self) -> Result<()> {
        if !self.rate.is_finite() {
            return Err(Error::invalid("rate must be a finite number"));
        }
        Ok(())
    }

    /// A finite rate whose product with the duration stays finite, so a
    /// mapped time always lands on a real number of microseconds.
    pub fn check(&self, duration_us: u64) -> Result<()> {
        self.validate()?;
        let seconds = duration_us as f64 / 1e6;
        if seconds > 0.0 {
            let limit = f64::MAX / seconds;
            if self.rate.abs() > limit {
                return Err(Error::invalid(format!(
                    "timing.rate {:e} is too large for a {seconds} s scene; keep |rate| at or below {limit:e}",
                    self.rate
                )));
            }
        }
        Ok(())
    }

    /// Map a presentation time onto the local time in `0..=duration_us`.
    ///
    /// A zero duration is a still image and maps to zero. A negative rate
    /// mirrors time; under `Once` that clamps to the start, so reverse playback
    /// belongs with `Cycle` or `PingPong`.
    pub fn map(&self, t_us: u64, duration_us: u64) -> u64 {
        if duration_us == 0 || !self.rate.is_finite() {
            return 0;
        }
        let d = duration_us as f64;
        let u = t_us as f64 * self.rate;
        let v = match self.loop_mode {
            LoopMode::Once => u.clamp(0.0, d),
            LoopMode::Cycle => u.rem_euclid(d),
            LoopMode::PingPong => {
                let p = u.rem_euclid(2.0 * d);
                if p <= d { p } else { 2.0 * d - p }
            }
        };
        v.round() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: u64 = 1_000_000;

    #[test]
    fn once_clamps_at_both_ends() {
        let timing = Timing::default();
        assert_eq!(timing.map(0, D), 0);
        assert_eq!(timing.map(D / 2, D), D / 2);
        assert_eq!(timing.map(D * 2, D), D);
    }

    #[test]
    fn cycle_wraps_at_the_duration() {
        let timing = Timing::new(LoopMode::Cycle, 1.0);
        assert_eq!(timing.map(D + 250_000, D), 250_000);
        assert_eq!(timing.map(D, D), 0);
    }

    #[test]
    fn pingpong_mirrors_the_second_half() {
        let timing = Timing::new(LoopMode::PingPong, 1.0);
        assert_eq!(timing.map(250_000, D), 250_000);
        assert_eq!(timing.map(1_250_000, D), 750_000);
        assert_eq!(timing.map(2_250_000, D), 250_000);
    }

    #[test]
    fn negative_rate_mirrors_under_cycle() {
        let timing = Timing::new(LoopMode::Cycle, -1.0);
        assert_eq!(timing.map(250_000, D), 750_000);
        assert_eq!(timing.map(750_000, D), 250_000);
        // Applying the map twice returns the original point.
        assert_eq!(timing.map(timing.map(250_000, D), D), 250_000);
    }

    #[test]
    fn half_rate_doubles_local_time() {
        let timing = Timing::new(LoopMode::Cycle, 0.5);
        assert_eq!(timing.map(500_000, D), 250_000);
    }

    #[test]
    fn an_extreme_rate_is_refused_for_its_duration() {
        let huge = Timing::new(LoopMode::Cycle, 1e308);
        assert!(huge.validate().is_ok(), "finiteness alone accepts it");
        let err = huge.check(2_000_000).unwrap_err();
        assert!(err.to_string().contains("timing.rate"), "{err}");
        assert!(err.to_string().contains("1e308"), "{err}");
        assert!(Timing::new(LoopMode::Cycle, 2.0).check(2_000_000).is_ok());
        assert!(Timing::new(LoopMode::Cycle, 0.0).check(2_000_000).is_ok());
        assert!(Timing::new(LoopMode::Cycle, -2.0).check(2_000_000).is_ok());
        assert!(huge.check(0).is_ok(), "a still image maps no time");
    }

    #[test]
    fn zero_duration_and_bad_rate_map_to_zero() {
        let timing = Timing::new(LoopMode::Cycle, 1.0);
        assert_eq!(timing.map(500, 0), 0);
        let bad = Timing::new(LoopMode::Cycle, f64::NAN);
        assert!(bad.validate().is_err());
        assert_eq!(bad.map(500, D), 0);
    }
}
