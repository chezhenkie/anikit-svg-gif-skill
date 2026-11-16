//! Tracks, keys and the prepared curve a sample reads.
//!
//! A key holds an absolute time, a value and the motion of the segment it
//! starts. Prepare compiles each segment once; sampling a track does a lookup,
//! one easing evaluation and one interpolation.

use anikit_core::{Error, Result};

use crate::ease::{Ease, EasePlan};

/// A typed value that can be interpolated and recombined.
pub trait Lerp: Clone + PartialEq + core::fmt::Debug {
    /// The additive zero used to combine weighted values.
    fn zero() -> Self;

    /// `self += other * k`, component by component.
    fn add_scaled(&mut self, other: &Self, k: f64);

    /// Interpolate, exact at both endpoints.
    fn lerp(&self, other: &Self, t: f64) -> Self {
        if t <= 0.0 {
            return self.clone();
        }
        if t >= 1.0 {
            return other.clone();
        }
        let mut out = Self::zero();
        out.add_scaled(self, 1.0 - t);
        out.add_scaled(other, t);
        out
    }

    /// Linear extrapolation for a layer weight, which may leave 0 to 1 when the
    /// binding allows overshoot. Unlike `lerp`, this never clamps.
    fn mix(&self, other: &Self, t: f64) -> Self {
        if t == 0.0 {
            return self.clone();
        }
        if t == 1.0 {
            return other.clone();
        }
        let mut out = Self::zero();
        out.add_scaled(self, 1.0 - t);
        out.add_scaled(other, t);
        out
    }

    /// Two values may share a track only when their kinds match.
    fn same_kind(&self, other: &Self) -> bool {
        let _ = other;
        true
    }

    /// The value as a scalar, for splines and ballistic motion.
    fn scalar(&self) -> Option<f64> {
        None
    }

    /// A scalar shaped like this value, preserving its kind.
    fn with_scalar(&self, value: f64) -> Option<Self> {
        let _ = value;
        None
    }

    /// False when any component is NaN or infinite.
    fn is_finite(&self) -> bool {
        true
    }
}

impl Lerp for f64 {
    fn zero() -> f64 {
        0.0
    }

    fn add_scaled(&mut self, other: &f64, k: f64) {
        *self += *other * k;
    }

    fn lerp(&self, other: &f64, t: f64) -> f64 {
        if t <= 0.0 {
            *self
        } else if t >= 1.0 {
            *other
        } else {
            *self + (*other - *self) * t
        }
    }

    fn mix(&self, other: &f64, t: f64) -> f64 {
        *self + (*other - *self) * t
    }

    fn scalar(&self) -> Option<f64> {
        Some(*self)
    }

    fn with_scalar(&self, value: f64) -> Option<f64> {
        Some(value)
    }

    fn is_finite(&self) -> bool {
        f64::is_finite(*self)
    }
}

impl Lerp for f32 {
    fn zero() -> f32 {
        0.0
    }

    fn add_scaled(&mut self, other: &f32, k: f64) {
        *self += *other * k as f32;
    }

    fn lerp(&self, other: &f32, t: f64) -> f32 {
        if t <= 0.0 {
            *self
        } else if t >= 1.0 {
            *other
        } else {
            *self + (*other - *self) * t as f32
        }
    }

    fn mix(&self, other: &f32, t: f64) -> f32 {
        *self + (*other - *self) * t as f32
    }

    fn scalar(&self) -> Option<f64> {
        Some(*self as f64)
    }

    fn with_scalar(&self, value: f64) -> Option<f32> {
        Some(value as f32)
    }

    fn is_finite(&self) -> bool {
        f32::is_finite(*self)
    }
}

/// How the interior of a multi-key track is smoothed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Smooth {
    /// Each segment uses the easing on its starting key.
    #[default]
    Eased,
    /// Linear segments become cubic Hermite with Catmull-Rom tangents.
    Auto,
    /// Hermite with Fritsch-Carlson tangents that cannot overshoot.
    Monotone,
}

/// A ballistic flight on one value axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ballistic {
    pub gravity: f64,
    pub v0: f64,
    pub plane: f64,
    pub restitution: f64,
    pub friction: f64,
    pub min_bounce: f64,
    pub max_speed: f64,
    pub contact_hold_us: u64,
    pub squash: f64,
    pub stretch: f64,
}

impl Ballistic {
    /// A drop onto an absolute plane with the cartoon gravity default.
    pub fn drop(plane: f64) -> Ballistic {
        Ballistic {
            gravity: 1400.0,
            v0: 0.0,
            plane,
            restitution: 0.5,
            friction: 0.3,
            min_bounce: 1.0,
            max_speed: 1000.0,
            contact_hold_us: 40_000,
            squash: 0.2,
            stretch: 0.15,
        }
    }

    pub fn gravity(mut self, gravity: f64) -> Ballistic {
        self.gravity = gravity;
        self
    }

    pub fn restitution(mut self, restitution: f64) -> Ballistic {
        self.restitution = restitution;
        self
    }

    pub fn contact_hold(mut self, hold_us: u64) -> Ballistic {
        self.contact_hold_us = hold_us;
        self
    }
}

/// The motion of one segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    Ease(Ease),
    Ballistic(Ballistic),
    Arc { bend: f64, ease: Ease },
}

impl Default for Motion {
    fn default() -> Motion {
        Motion::Ease(Ease::Linear)
    }
}

impl Motion {
    pub fn is_hold(&self) -> bool {
        matches!(self, Motion::Ease(Ease::Hold))
    }
}

/// One key: an absolute time, a value and the motion it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Key<T> {
    pub t_us: u64,
    pub value: T,
    pub motion: Motion,
    /// Explicit outgoing tangent in value units per second, scalar tracks only.
    pub tangent: Option<f64>,
}

impl<T> Key<T> {
    pub fn new(t_us: u64, value: T) -> Key<T> {
        Key {
            t_us,
            value,
            motion: Motion::Ease(Ease::Linear),
            tangent: None,
        }
    }

    pub fn ease(mut self, ease: Ease) -> Key<T> {
        self.motion = Motion::Ease(ease);
        self
    }

    pub fn motion(mut self, motion: Motion) -> Key<T> {
        self.motion = motion;
        self
    }

    pub fn hold(mut self) -> Key<T> {
        self.motion = Motion::Ease(Ease::Hold);
        self
    }

    pub fn tangent(mut self, tangent: f64) -> Key<T> {
        self.tangent = Some(tangent);
        self
    }
}

/// Keys in time order, plus the smoothing rule for linear segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Track<T> {
    pub keys: Vec<Key<T>>,
    pub smooth: Smooth,
}

impl<T> Track<T> {
    pub fn new(keys: Vec<Key<T>>) -> Track<T> {
        Track {
            keys,
            smooth: Smooth::Eased,
        }
    }

    pub fn constant(value: T) -> Track<T> {
        Track::new(vec![Key::new(0, value)])
    }

    pub fn smooth(mut self, smooth: Smooth) -> Track<T> {
        self.smooth = smooth;
        self
    }
}

impl<T: Lerp> Track<T> {
    pub fn prepare(&self) -> Result<Curve<T>> {
        self.prepare_named("track")
    }

    /// Compile, naming the track in every failure.
    pub fn prepare_named(&self, name: &str) -> Result<Curve<T>> {
        if self.keys.is_empty() {
            return Err(Error::invalid(format!(
                "{name}: a track needs at least one key"
            )));
        }
        let mut run = 1usize;
        for i in 1..self.keys.len() {
            let previous = &self.keys[i - 1];
            let key = &self.keys[i];
            if key.t_us < previous.t_us {
                return Err(Error::invalid(format!(
                    "{name}: keys must be in time order, {} follows {}",
                    key.t_us, previous.t_us
                )));
            }
            if key.t_us == previous.t_us {
                run += 1;
                if run > 2 {
                    return Err(Error::invalid(format!(
                        "{name}: three keys share the time {}",
                        key.t_us
                    )));
                }
                if !previous.motion.is_hold() {
                    return Err(Error::invalid(format!(
                        "{name}: two keys share the time {} but the earlier one does not hold",
                        key.t_us
                    )));
                }
            } else {
                run = 1;
            }
        }
        let first = &self.keys[0].value;
        for key in &self.keys {
            if !first.same_kind(&key.value) {
                return Err(Error::invalid(format!(
                    "{name}: a track cannot mix value kinds"
                )));
            }
            if !key.value.is_finite() {
                return Err(Error::invalid(format!(
                    "{name}: key at {} has a value that is not finite",
                    key.t_us
                )));
            }
        }
        if self.smooth == Smooth::Monotone && first.scalar().is_none() {
            return Err(Error::invalid(format!(
                "{name}: monotone smoothing needs a scalar track"
            )));
        }

        let tangents = if self.smooth == Smooth::Eased || self.keys.len() == 1 {
            Vec::new()
        } else {
            self.tangents(name)?
        };

        let mut segments = Vec::with_capacity(self.keys.len().saturating_sub(1));
        for i in 0..self.keys.len().saturating_sub(1) {
            let from = &self.keys[i];
            let to = &self.keys[i + 1];
            let span = to.t_us - from.t_us;
            if span == 0 {
                segments.push(Segment {
                    start_us: from.t_us,
                    end_us: to.t_us,
                    from: from.value.clone(),
                    to: to.value.clone(),
                    kind: SegmentKind::Hold,
                });
                continue;
            }
            let kind = match from.motion {
                Motion::Ease(Ease::Hold) => SegmentKind::Hold,
                Motion::Ease(ease) if self.smooth != Smooth::Eased && ease == Ease::Linear => {
                    let span_s = span as f64 / 1e6;
                    let m0 = scale(&tangents[i], span_s);
                    let m1 = scale(&tangents[i + 1], span_s);
                    SegmentKind::Hermite { m0, m1 }
                }
                Motion::Ease(ease) => {
                    let plan = ease.prepare(span).map_err(|error| {
                        Error::invalid(format!("{name}: key at {}: {error}", from.t_us))
                    })?;
                    if let Some(rest) = plan.spring_rest()
                        && let (Some(a), Some(b)) = (from.value.scalar(), to.value.scalar())
                    {
                        let travel = (b - a).abs();
                        if travel * rest.distance > rest.rest_distance
                            || travel * rest.speed > rest.rest_speed
                        {
                            return Err(Error::invalid(format!(
                                "{name}: spring at {} is still moving at its settle time",
                                from.t_us
                            )));
                        }
                    }
                    SegmentKind::Eased(plan)
                }
                Motion::Ballistic(ballistic) => {
                    let start = from.value.scalar().ok_or_else(|| {
                        Error::invalid(format!("{name}: ballistic motion needs a scalar track"))
                    })?;
                    SegmentKind::Ballistic(BallisticPlan::prepare(ballistic, start).map_err(
                        |error| Error::invalid(format!("{name}: key at {}: {error}", from.t_us)),
                    )?)
                }
                Motion::Arc { bend, ease } => {
                    let a = from.value.scalar().ok_or_else(|| {
                        Error::invalid(format!("{name}: arc motion needs a scalar track"))
                    })?;
                    let b = to.value.scalar().ok_or_else(|| {
                        Error::invalid(format!("{name}: arc motion needs a scalar track"))
                    })?;
                    if !bend.is_finite() {
                        return Err(Error::invalid(format!("{name}: arc bend must be finite")));
                    }
                    let control = from
                        .value
                        .with_scalar(0.5 * (a + b) + bend * (b - a))
                        .ok_or_else(|| {
                            Error::invalid(format!("{name}: arc motion needs a scalar track"))
                        })?;
                    let plan = ease.prepare(span).map_err(|error| {
                        Error::invalid(format!("{name}: key at {}: {error}", from.t_us))
                    })?;
                    SegmentKind::Arc {
                        control,
                        ease: plan,
                    }
                }
            };
            segments.push(Segment {
                start_us: from.t_us,
                end_us: to.t_us,
                from: from.value.clone(),
                to: to.value.clone(),
                kind,
            });
        }

        Ok(Curve {
            start_us: self.keys[0].t_us,
            end_us: self.keys[self.keys.len() - 1].t_us,
            keys: self.keys.clone(),
            segments,
        })
    }

    fn tangents(&self, name: &str) -> Result<Vec<T>> {
        let n = self.keys.len();
        let mut tangents = Vec::with_capacity(n);
        for (i, key) in self.keys.iter().enumerate() {
            if let Some(explicit) = key.tangent {
                if !explicit.is_finite() {
                    return Err(Error::invalid(format!(
                        "{name}: tangent at {} must be finite",
                        key.t_us
                    )));
                }
                let tangent = key.value.with_scalar(explicit).ok_or_else(|| {
                    Error::invalid(format!("{name}: explicit tangents need a scalar track"))
                })?;
                tangents.push(tangent);
                continue;
            }
            let tangent = if n == 1 {
                T::zero()
            } else if i == 0 {
                let dt = (self.keys[1].t_us - key.t_us) as f64 / 1e6;
                if dt <= 0.0 {
                    T::zero()
                } else {
                    difference(&self.keys[1].value, &key.value, 1.0 / dt)
                }
            } else if i == n - 1 {
                let previous = &self.keys[i - 1];
                let dt = (key.t_us - previous.t_us) as f64 / 1e6;
                if dt <= 0.0 {
                    T::zero()
                } else {
                    difference(&key.value, &previous.value, 1.0 / dt)
                }
            } else {
                let previous = &self.keys[i - 1];
                let next = &self.keys[i + 1];
                if previous.t_us == key.t_us || next.t_us == key.t_us {
                    T::zero()
                } else {
                    let dt = (next.t_us - previous.t_us) as f64 / 1e6;
                    difference(&next.value, &previous.value, 1.0 / dt)
                }
            };
            tangents.push(tangent);
        }
        if self.smooth == Smooth::Monotone {
            self.monotone_tangents(&mut tangents, name)?;
        }
        Ok(tangents)
    }

    fn monotone_tangents(&self, tangents: &mut [T], name: &str) -> Result<()> {
        let values: Vec<f64> = self
            .keys
            .iter()
            .map(|key| key.value.scalar())
            .collect::<Option<Vec<f64>>>()
            .ok_or_else(|| {
                Error::invalid(format!("{name}: monotone smoothing needs a scalar track"))
            })?;
        let mut slopes: Vec<f64> = tangents
            .iter()
            .map(|tangent| tangent.scalar())
            .collect::<Option<Vec<f64>>>()
            .ok_or_else(|| {
                Error::invalid(format!("{name}: monotone smoothing needs a scalar track"))
            })?;
        for i in 0..self.keys.len().saturating_sub(1) {
            let h = (self.keys[i + 1].t_us - self.keys[i].t_us) as f64 / 1e6;
            if h <= 0.0 {
                continue;
            }
            let secant = (values[i + 1] - values[i]) / h;
            if secant == 0.0 {
                slopes[i] = 0.0;
                slopes[i + 1] = 0.0;
                continue;
            }
            let alpha = slopes[i] / secant;
            let beta = slopes[i + 1] / secant;
            let magnitude = alpha * alpha + beta * beta;
            if magnitude > 9.0 {
                let tau = 3.0 / magnitude.sqrt();
                slopes[i] = tau * alpha * secant;
                slopes[i + 1] = tau * beta * secant;
            }
        }
        for (tangent, slope) in tangents.iter_mut().zip(slopes) {
            *tangent = self.keys[0].value.with_scalar(slope).ok_or_else(|| {
                Error::invalid(format!("{name}: monotone smoothing needs a scalar track"))
            })?;
        }
        Ok(())
    }
}

fn difference<T: Lerp>(next: &T, previous: &T, k: f64) -> T {
    let mut out = T::zero();
    out.add_scaled(next, k);
    out.add_scaled(previous, -k);
    out
}

fn scale<T: Lerp>(value: &T, k: f64) -> T {
    let mut out = T::zero();
    out.add_scaled(value, k);
    out
}

/// A prepared curve. Sampling is total and allocation free.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve<T> {
    start_us: u64,
    end_us: u64,
    keys: Vec<Key<T>>,
    segments: Vec<Segment<T>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Segment<T> {
    start_us: u64,
    end_us: u64,
    from: T,
    to: T,
    kind: SegmentKind<T>,
}

#[derive(Debug, Clone, PartialEq)]
enum SegmentKind<T> {
    Hold,
    Eased(EasePlan),
    Hermite { m0: T, m1: T },
    Ballistic(BallisticPlan),
    Arc { control: T, ease: EasePlan },
}

impl<T: Lerp> Curve<T> {
    pub fn start_us(&self) -> u64 {
        self.start_us
    }

    pub fn end_us(&self) -> u64 {
        self.end_us
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    /// The value at a time. Exact at every key time, clamps outside the range.
    pub fn sample(&self, t_us: u64) -> T {
        if t_us <= self.start_us {
            return self.keys[0].value.clone();
        }
        if t_us >= self.end_us {
            return self.keys[self.keys.len() - 1].value.clone();
        }
        let index = self.keys.partition_point(|key| key.t_us <= t_us) - 1;
        let key = &self.keys[index];
        if key.t_us == t_us {
            return key.value.clone();
        }
        let segment = &self.segments[index];
        let span = (segment.end_us - segment.start_us) as f64;
        let u = (t_us - segment.start_us) as f64 / span;
        match &segment.kind {
            SegmentKind::Hold => segment.from.clone(),
            SegmentKind::Eased(plan) => segment.from.lerp(&segment.to, plan.eval(u)),
            SegmentKind::Hermite { m0, m1 } => {
                let u2 = u * u;
                let u3 = u2 * u;
                let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
                let h10 = u3 - 2.0 * u2 + u;
                let h01 = -2.0 * u3 + 3.0 * u2;
                let h11 = u3 - u2;
                let mut out = T::zero();
                out.add_scaled(&segment.from, h00);
                out.add_scaled(&segment.to, h01);
                out.add_scaled(m0, h10);
                out.add_scaled(m1, h11);
                out
            }
            SegmentKind::Ballistic(plan) => segment
                .from
                .with_scalar(plan.value(t_us - segment.start_us))
                .unwrap_or_else(|| segment.to.clone()),
            SegmentKind::Arc { control, ease } => {
                let p = ease.eval(u);
                let mut out = T::zero();
                let one = 1.0 - p;
                out.add_scaled(&segment.from, one * one);
                out.add_scaled(control, 2.0 * one * p);
                out.add_scaled(&segment.to, p * p);
                out
            }
        }
    }

    /// Contact times of every ballistic segment, for the frame plan.
    pub fn contacts(&self) -> Vec<u64> {
        let mut times = Vec::new();
        for segment in &self.segments {
            if let SegmentKind::Ballistic(plan) = &segment.kind {
                times.extend(plan.contacts.iter().map(|offset| segment.start_us + offset));
            }
        }
        times
    }
}

/// A compiled ballistic flight and its contacts.
#[derive(Debug, Clone, PartialEq)]
struct BallisticPlan {
    start: f64,
    phases: Vec<Phase>,
    contacts: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct Phase {
    start_us: u64,
    end_us: u64,
    kind: PhaseKind,
}

#[derive(Debug, Clone, PartialEq)]
enum PhaseKind {
    Flight { p0: f64, v0: f64, a: f64 },
    Hold { p: f64 },
}

impl BallisticPlan {
    fn prepare(ballistic: Ballistic, start: f64) -> Result<BallisticPlan> {
        let values = [
            ballistic.gravity,
            ballistic.v0,
            ballistic.plane,
            ballistic.restitution,
            ballistic.friction,
            ballistic.min_bounce,
            ballistic.max_speed,
            ballistic.squash,
            ballistic.stretch,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(Error::invalid("ballistic parameters must be finite"));
        }
        if ballistic.gravity <= 0.0 {
            return Err(Error::invalid("ballistic gravity must be greater than 0"));
        }
        if !(0.0..=1.0).contains(&ballistic.restitution) {
            return Err(Error::invalid("ballistic restitution must be in 0 to 1"));
        }
        if !(0.0..=1.0).contains(&ballistic.friction) {
            return Err(Error::invalid("ballistic friction must be in 0 to 1"));
        }
        if ballistic.min_bounce < 0.0 {
            return Err(Error::invalid("ballistic min_bounce must be 0 or more"));
        }
        if ballistic.max_speed <= 0.0 {
            return Err(Error::invalid("ballistic max_speed must be greater than 0"));
        }
        let plane = ballistic.plane;
        let a = if plane >= start {
            ballistic.gravity
        } else {
            -ballistic.gravity
        };
        let mut p = start;
        let mut v = ballistic
            .v0
            .clamp(-ballistic.max_speed, ballistic.max_speed);
        let mut t_us = 0u64;
        let mut phases = Vec::new();
        let mut contacts = Vec::new();
        for _ in 0..64 {
            if (p - plane).abs() < 1e-9 && v.abs() < 1e-9 {
                break;
            }
            let hit_s = time_to_plane(p, v, a, plane)?;
            let dur_us = (hit_s * 1e6).round().max(1.0) as u64;
            phases.push(Phase {
                start_us: t_us,
                end_us: t_us + dur_us,
                kind: PhaseKind::Flight { p0: p, v0: v, a },
            });
            t_us += dur_us;
            let impact = v + a * hit_s;
            p = plane;
            contacts.push(t_us);
            if ballistic.contact_hold_us > 0 {
                phases.push(Phase {
                    start_us: t_us,
                    end_us: t_us + ballistic.contact_hold_us,
                    kind: PhaseKind::Hold { p: plane },
                });
                t_us += ballistic.contact_hold_us;
            }
            let rebound =
                (-impact * ballistic.restitution).clamp(-ballistic.max_speed, ballistic.max_speed);
            let height = rebound * rebound / (2.0 * ballistic.gravity);
            if height < ballistic.min_bounce || rebound.abs() < 1e-6 {
                break;
            }
            v = rebound;
        }
        Ok(BallisticPlan {
            start,
            phases,
            contacts,
        })
    }

    fn value(&self, elapsed_us: u64) -> f64 {
        let Some(first) = self.phases.first() else {
            return self.start;
        };
        if elapsed_us < first.start_us {
            return match &first.kind {
                PhaseKind::Flight { p0, .. } => *p0,
                PhaseKind::Hold { p } => *p,
            };
        }
        for phase in &self.phases {
            if elapsed_us < phase.end_us {
                let dt = (elapsed_us - phase.start_us) as f64 / 1e6;
                return match &phase.kind {
                    PhaseKind::Flight { p0, v0, a } => p0 + v0 * dt + 0.5 * a * dt * dt,
                    PhaseKind::Hold { p } => *p,
                };
            }
        }
        match self.phases.last().map(|phase| &phase.kind) {
            Some(PhaseKind::Hold { p }) => *p,
            Some(PhaseKind::Flight { p0, v0, a }) => {
                let phase = self.phases.last().unwrap();
                let dt = (phase.end_us - phase.start_us) as f64 / 1e6;
                p0 + v0 * dt + 0.5 * a * dt * dt
            }
            None => self.start,
        }
    }
}

/// The first positive time at which the parabola reaches the plane.
fn time_to_plane(p: f64, v: f64, a: f64, plane: f64) -> Result<f64> {
    let offset = p - plane;
    if a.abs() < 1e-12 {
        if v.abs() < 1e-12 {
            return Err(Error::invalid("ballistic never reaches its plane"));
        }
        let t = -offset / v;
        if t > 1e-6 {
            return Ok(t);
        }
        return Err(Error::invalid("ballistic never reaches its plane"));
    }
    let discriminant = v * v - 2.0 * a * offset;
    if discriminant < 0.0 {
        return Err(Error::invalid("ballistic never reaches its plane"));
    }
    let root = discriminant.sqrt();
    let first = (-v + root) / a;
    let second = (-v - root) / a;
    let mut best: Option<f64> = None;
    for t in [first, second] {
        if t > 1e-6 && t.is_finite() && best.is_none_or(|current| t < current) {
            best = Some(t);
        }
    }
    best.ok_or_else(|| Error::invalid("ballistic never reaches its plane"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ease::{Spring, StepAt};
    use anikit_time::LoopMode;

    fn number_track(keys: Vec<Key<f64>>) -> Track<f64> {
        Track::new(keys)
    }

    #[test]
    fn sample_is_exact_at_every_key_time() {
        let track = number_track(vec![
            Key::new(0, 0.0),
            Key::new(500_000, 10.0),
            Key::new(1_000_000, 4.0),
        ]);
        let curve = track.prepare().unwrap();
        assert_eq!(curve.sample(0), 0.0);
        assert_eq!(curve.sample(500_000), 10.0);
        assert_eq!(curve.sample(1_000_000), 4.0);
    }

    #[test]
    fn values_clamp_outside_the_key_range() {
        let curve = number_track(vec![Key::new(100, 3.0), Key::new(200, 7.0)])
            .prepare()
            .unwrap();
        assert_eq!(curve.sample(0), 3.0);
        assert_eq!(curve.sample(50), 3.0);
        assert_eq!(curve.sample(1_000), 7.0);
    }

    #[test]
    fn a_hold_key_keeps_its_value_until_the_next_key() {
        let curve = number_track(vec![
            Key::new(0, 1.0).hold(),
            Key::new(500_000, 9.0).hold(),
            Key::new(1_000_000, 2.0),
        ])
        .prepare()
        .unwrap();
        assert_eq!(curve.sample(499_999), 1.0);
        assert_eq!(curve.sample(500_000), 9.0);
        assert_eq!(curve.sample(999_999), 9.0);
        assert_eq!(curve.sample(1_000_000), 2.0);
    }

    #[test]
    fn an_eased_segment_changes_only_its_own_interval() {
        let keys = vec![
            Key::new(0, 0.0),
            Key::new(500_000, 10.0),
            Key::new(1_000_000, 20.0),
        ];
        let plain = number_track(keys.clone()).prepare().unwrap();
        let mut eased = keys;
        eased[0] = eased[0].clone().ease(Ease::cubic(0.42, 0.0, 1.0, 1.0));
        let eased = number_track(eased).prepare().unwrap();
        assert!((plain.sample(250_000) - eased.sample(250_000)).abs() > 0.1);
        assert_eq!(plain.sample(750_000), eased.sample(750_000));
    }

    #[test]
    fn the_last_key_easing_is_ignored() {
        let curve = number_track(vec![
            Key::new(0, 0.0),
            Key::new(1_000_000, 1.0).ease(Ease::cubic(2.0, 0.0, 1.0, 1.0)),
        ])
        .prepare()
        .unwrap();
        assert_eq!(curve.sample(500_000), 0.5);
    }

    #[test]
    fn unsorted_keys_are_rejected() {
        let track = number_track(vec![Key::new(100, 0.0), Key::new(0, 1.0)]);
        assert!(track.prepare().is_err());
    }

    #[test]
    fn a_duplicate_needs_an_earlier_hold() {
        let legal = number_track(vec![
            Key::new(0, 0.0),
            Key::new(500_000, 1.0).hold(),
            Key::new(500_000, 2.0),
        ]);
        let curve = legal.prepare().unwrap();
        assert!((curve.sample(499_999) - 1.0).abs() < 1e-3);
        assert_eq!(curve.sample(500_000), 2.0);

        let illegal = number_track(vec![
            Key::new(0, 0.0),
            Key::new(500_000, 1.0),
            Key::new(500_000, 2.0),
        ]);
        assert!(illegal.prepare().is_err());

        let three = number_track(vec![
            Key::new(0, 0.0),
            Key::new(500_000, 1.0).hold(),
            Key::new(500_000, 2.0).hold(),
            Key::new(500_000, 3.0),
        ]);
        assert!(three.prepare().is_err());
    }

    #[test]
    fn one_key_is_constant() {
        let curve = number_track(vec![Key::new(700, 5.0)]).prepare().unwrap();
        assert_eq!(curve.sample(0), 5.0);
        assert_eq!(curve.sample(700), 5.0);
        assert_eq!(curve.sample(9_999), 5.0);
    }

    #[test]
    fn a_smooth_track_is_continuous_at_the_keys() {
        let track = number_track(vec![
            Key::new(0, 0.0),
            Key::new(500_000, 10.0),
            Key::new(1_000_000, 0.0),
        ])
        .smooth(Smooth::Auto);
        let curve = track.prepare().unwrap();
        assert_eq!(curve.sample(500_000), 10.0);
        let before = curve.sample(499_999);
        let after = curve.sample(500_001);
        assert!((before - after).abs() < 1e-3, "{before} {after}");
    }

    #[test]
    fn monotone_smoothing_never_overshoots() {
        let curve = number_track(vec![
            Key::new(0, 0.0),
            Key::new(400_000, 10.0),
            Key::new(800_000, 10.1),
            Key::new(1_200_000, 30.0),
        ])
        .smooth(Smooth::Monotone)
        .prepare()
        .unwrap();
        for i in 0..=1200 {
            let value = curve.sample(i * 1000);
            assert!(
                (0.0..=30.0).contains(&value),
                "t {} value {value}",
                i * 1000
            );
        }
    }

    #[test]
    fn monotone_rejects_non_scalars() {
        use crate::value::{Transform, Value};
        let scalar =
            Track::new(vec![Key::new(0, 0.5f32), Key::new(1, 1.0f32)]).smooth(Smooth::Monotone);
        assert!(scalar.prepare().is_ok());
        let bad = Track::new(vec![
            Key::new(0, Value::Transform(Transform::IDENTITY)),
            Key::new(1, Value::Transform(Transform::translate(1.0, 0.0))),
        ])
        .smooth(Smooth::Monotone);
        assert!(bad.prepare().is_err());
    }

    #[test]
    fn a_spring_that_is_still_moving_is_rejected() {
        let spring = Spring::new(100.0, 4.0, 1.0, 200_000);
        let track = number_track(vec![
            Key::new(0, 0.0).ease(Ease::Spring(spring)),
            Key::new(1_000_000, 100.0),
        ]);
        assert!(track.prepare().is_err());
    }

    #[test]
    fn a_settled_spring_is_accepted() {
        let spring = Spring::new(100.0, 10.0, 1.0, 1_000_000);
        let track = number_track(vec![
            Key::new(0, 0.0).ease(Ease::Spring(spring)),
            Key::new(1_000_000, 1.0),
        ]);
        let curve = track.prepare().unwrap();
        assert_eq!(curve.sample(0), 0.0);
        assert_eq!(curve.sample(1_000_000), 1.0);
    }

    #[test]
    fn a_spring_segment_holds_after_it_settles() {
        let spring = Spring::new(100.0, 10.0, 1.0, 500_000);
        let curve = number_track(vec![
            Key::new(0, 0.0).ease(Ease::Spring(spring)),
            Key::new(1_000_000, 1.0),
        ])
        .prepare()
        .unwrap();
        assert_eq!(curve.sample(500_000), 1.0);
        assert_eq!(curve.sample(750_000), 1.0);
    }

    #[test]
    fn ballistic_drop_reaches_the_plane_on_time_and_holds_contact() {
        let drop = Ballistic::drop(100.0).contact_hold(40_000);
        let curve = number_track(vec![
            Key::new(0, 0.0).motion(Motion::Ballistic(drop)),
            Key::new(1_000_000, 100.0),
        ])
        .prepare()
        .unwrap();
        let contacts = curve.contacts();
        assert!(!contacts.is_empty());
        let expected = (2.0 * 100.0 / 1400.0f64).sqrt();
        let first = contacts[0] as f64 / 1e6;
        assert!(
            (first - expected).abs() < 0.04,
            "{first} against {expected}"
        );
        // The contact is held, so the value is the plane on both sides.
        assert_eq!(curve.sample(contacts[0]), 100.0);
        assert_eq!(curve.sample(contacts[0] + 20_000), 100.0);
    }

    #[test]
    fn bounce_heights_decay_by_restitution_squared() {
        let drop = Ballistic::drop(100.0).contact_hold(0).restitution(0.5);
        let curve = number_track(vec![
            Key::new(0, 0.0).motion(Motion::Ballistic(drop)),
            Key::new(3_000_000, 100.0),
        ])
        .prepare()
        .unwrap();
        let contacts = curve.contacts();
        assert!(contacts.len() >= 2, "{contacts:?}");
        // Peak heights are measured from the midpoint between contacts.
        let mut peaks = Vec::new();
        for pair in contacts.windows(2) {
            let start = pair[0];
            let end = pair[1];
            let mut highest = f64::INFINITY;
            let step = ((end - start) / 200).max(1);
            for i in 0..=200 {
                let value = curve.sample(start + i * step);
                if value < highest {
                    highest = value;
                }
            }
            peaks.push(highest);
        }
        let first_height = 100.0 - peaks[0];
        let second_height = 100.0 - peaks[1];
        let ratio = second_height / first_height;
        assert!((ratio - 0.25).abs() < 0.05, "ratio {ratio}");
    }

    #[test]
    fn bounce_intervals_decay_by_restitution() {
        let drop = Ballistic::drop(100.0).contact_hold(0).restitution(0.5);
        let curve = number_track(vec![
            Key::new(0, 0.0).motion(Motion::Ballistic(drop)),
            Key::new(3_000_000, 100.0),
        ])
        .prepare()
        .unwrap();
        let contacts = curve.contacts();
        assert!(contacts.len() >= 3);
        let first = (contacts[1] - contacts[0]) as f64;
        let second = (contacts[2] - contacts[1]) as f64;
        assert!((second / first - 0.5).abs() < 0.06, "{}", second / first);
    }

    #[test]
    fn an_arc_bends_the_middle_and_keeps_the_ends() {
        let curve = number_track(vec![
            Key::new(0, 0.0).motion(Motion::Arc {
                bend: 0.1,
                ease: Ease::Linear,
            }),
            Key::new(1_000_000, 100.0),
        ])
        .prepare()
        .unwrap();
        assert_eq!(curve.sample(0), 0.0);
        assert_eq!(curve.sample(1_000_000), 100.0);
        let middle = curve.sample(500_000);
        assert!((54.9..60.0).contains(&middle), "middle {middle}");
    }

    #[test]
    fn a_track_cannot_mix_kinds() {
        use crate::value::Value;
        let track = Track::new(vec![
            Key::new(0, Value::Number(0.0)),
            Key::new(1, Value::Opacity(1.0)),
        ]);
        assert!(track.prepare().is_err());
    }

    #[test]
    fn reverse_sampling_returns_the_same_values() {
        let curve = number_track(vec![
            Key::new(0, 0.0),
            Key::new(250_000, 4.0),
            Key::new(500_000, 1.0),
            Key::new(1_000_000, 9.0),
        ])
        .prepare()
        .unwrap();
        let times = [0, 100_000, 300_000, 700_000, 1_000_000];
        let forward: Vec<f64> = times.iter().map(|t| curve.sample(*t)).collect();
        let backward: Vec<f64> = times.iter().rev().map(|t| curve.sample(*t)).rev().collect();
        assert_eq!(forward, backward);
        let timing = crate::Timing::new(LoopMode::Cycle, -1.0);
        for t in [100_000u64, 300_000, 700_000, 900_000] {
            assert_eq!(timing.map(timing.map(t, 1_000_000), 1_000_000), t);
        }
    }

    #[test]
    fn steps_are_discrete_by_intent() {
        let curve = number_track(vec![
            Key::new(0, 0.0).ease(Ease::steps(4, StepAt::End)),
            Key::new(1_000_000, 1.0),
        ])
        .prepare()
        .unwrap();
        assert_eq!(curve.sample(0), 0.0);
        assert_eq!(curve.sample(249_999), 0.0);
        assert_eq!(curve.sample(250_000), 0.25);
        assert_eq!(curve.sample(999_999), 0.75);
        assert_eq!(curve.sample(1_000_000), 1.0);
    }
}
