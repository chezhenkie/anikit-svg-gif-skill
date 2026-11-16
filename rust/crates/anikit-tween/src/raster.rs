//! Raster plans: source, fit, crop, zoom and blend for a time.
//!
//! The crate never decodes and never resamples. A prepared plan names the work
//! and leaves the pixels to the renderer, which keeps the only per-frame
//! hotspot outside the sampler.

use anikit_core::{Error, Result};

/// A decoded image the renderer owns, named by a small id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RasterRef(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Cover,
    Contain,
}

/// A crop in normalised source coordinates, from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const FULL: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Cut,
    Crossfade,
}

/// One raster key. The renderer executes it; the crate only sequences it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterKey {
    pub t_us: u64,
    pub source: RasterRef,
    pub fit: FitMode,
    pub crop: Rect,
    pub zoom: f64,
    pub blend: Blend,
}

impl RasterKey {
    pub fn new(t_us: u64, source: RasterRef) -> RasterKey {
        RasterKey {
            t_us,
            source,
            fit: FitMode::Cover,
            crop: Rect::FULL,
            zoom: 1.0,
            blend: Blend::Crossfade,
        }
    }

    pub fn fit(mut self, fit: FitMode) -> RasterKey {
        self.fit = fit;
        self
    }

    pub fn crop(mut self, crop: Rect) -> RasterKey {
        self.crop = crop;
        self
    }

    pub fn zoom(mut self, zoom: f64) -> RasterKey {
        self.zoom = zoom;
        self
    }

    pub fn cut(mut self) -> RasterKey {
        self.blend = Blend::Cut;
        self
    }
}

/// Raster keys in time order.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterTrack {
    pub keys: Vec<RasterKey>,
}

impl RasterTrack {
    pub fn new(keys: Vec<RasterKey>) -> RasterTrack {
        RasterTrack { keys }
    }

    pub fn prepare(&self, name: &str) -> Result<RasterPlan> {
        if self.keys.is_empty() {
            return Err(Error::invalid(format!(
                "{name}: a raster track needs at least one key"
            )));
        }
        for i in 1..self.keys.len() {
            if self.keys[i].t_us <= self.keys[i - 1].t_us {
                return Err(Error::invalid(format!(
                    "{name}: raster keys must be in strictly increasing time order"
                )));
            }
        }
        for key in &self.keys {
            if !key.zoom.is_finite() || key.zoom <= 0.0 {
                return Err(Error::invalid(format!(
                    "{name}: raster zoom at {} must be greater than 0",
                    key.t_us
                )));
            }
            if !key.crop.x.is_finite()
                || !key.crop.y.is_finite()
                || !key.crop.w.is_finite()
                || !key.crop.h.is_finite()
                || key.crop.w < 0.0
                || key.crop.h < 0.0
            {
                return Err(Error::invalid(format!(
                    "{name}: raster crop at {} is not a valid rectangle",
                    key.t_us
                )));
            }
        }
        Ok(RasterPlan {
            keys: self.keys.clone(),
        })
    }
}

/// A prepared raster track. A step names the pair the renderer draws.
#[derive(Debug, Clone, PartialEq)]
pub struct RasterPlan {
    keys: Vec<RasterKey>,
}

/// The raster work at one time: draw `from`, and crossfade toward `to` by `mix`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterStep {
    pub from: RasterKey,
    pub to: RasterKey,
    pub mix: f32,
}

impl RasterStep {
    /// The source drawn at full strength; the crossfade target is `to`.
    pub fn source(&self) -> RasterRef {
        self.from.source
    }
}

impl RasterPlan {
    pub fn start_us(&self) -> u64 {
        self.keys[0].t_us
    }

    pub fn end_us(&self) -> u64 {
        self.keys[self.keys.len() - 1].t_us
    }

    pub fn sample(&self, t_us: u64) -> RasterStep {
        if t_us <= self.keys[0].t_us {
            return RasterStep {
                from: self.keys[0],
                to: self.keys[0],
                mix: 0.0,
            };
        }
        let last = self.keys.len() - 1;
        if t_us >= self.keys[last].t_us {
            return RasterStep {
                from: self.keys[last],
                to: self.keys[last],
                mix: 0.0,
            };
        }
        let index = self.keys.partition_point(|key| key.t_us <= t_us) - 1;
        let from = self.keys[index];
        let to = self.keys[index + 1];
        if from.t_us == t_us || from.blend == Blend::Cut {
            return RasterStep { from, to, mix: 0.0 };
        }
        let span = (to.t_us - from.t_us) as f32;
        RasterStep {
            from,
            to,
            mix: (t_us - from.t_us) as f32 / span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crossfade_reports_the_midpoint() {
        let plan = RasterTrack::new(vec![
            RasterKey::new(0, RasterRef(1)),
            RasterKey::new(1_000_000, RasterRef(2)),
        ])
        .prepare("photos")
        .unwrap();
        let step = plan.sample(500_000);
        assert_eq!(step.from.source, RasterRef(1));
        assert_eq!(step.to.source, RasterRef(2));
        assert!((step.mix - 0.5).abs() < 1e-6);
        assert_eq!(plan.sample(0).mix, 0.0);
        assert_eq!(plan.sample(2_000_000).source(), RasterRef(2));
    }

    #[test]
    fn a_cut_holds_the_first_source_until_the_next_key() {
        let plan = RasterTrack::new(vec![
            RasterKey::new(0, RasterRef(1)).cut(),
            RasterKey::new(1_000_000, RasterRef(2)),
        ])
        .prepare("photos")
        .unwrap();
        assert_eq!(plan.sample(999_999).mix, 0.0);
        assert_eq!(plan.sample(999_999).from.source, RasterRef(1));
        assert_eq!(plan.sample(1_000_000).mix, 0.0);
    }

    #[test]
    fn invalid_zoom_and_crop_are_rejected() {
        let bad_zoom = RasterTrack::new(vec![RasterKey::new(0, RasterRef(1)).zoom(0.0)]);
        assert!(bad_zoom.prepare("photos").is_err());
        let bad_crop = RasterTrack::new(vec![
            RasterKey::new(0, RasterRef(1)).crop(Rect::new(0.0, 0.0, -1.0, 1.0)),
        ]);
        assert!(bad_crop.prepare("photos").is_err());
    }

    #[test]
    fn keys_must_increase_in_time() {
        let track = RasterTrack::new(vec![
            RasterKey::new(1_000, RasterRef(1)),
            RasterKey::new(1_000, RasterRef(2)),
        ]);
        assert!(track.prepare("photos").is_err());
    }
}
