//! Easing curves and their compiled form.
//!
//! A segment's easing belongs to the key it leaves. Compilation happens once
//! per segment at prepare: bezier coefficients and their 11-point table, the
//! step count, or the closed-form spring. Evaluation is one lookup, one solve
//! or one exponential per sample.

use anikit_core::{Error, Result};

/// Where a stepped easing jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAt {
    Start,
    End,
    None,
    Both,
}

/// Parameters of a closed-form spring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub stiffness: f64,
    pub damping: f64,
    pub mass: f64,
    pub v0: f64,
    pub settle_us: u64,
    pub rest_delta: f64,
    pub rest_speed: f64,
}

impl Spring {
    /// A spring at rest with the default pixel thresholds.
    pub fn new(stiffness: f64, damping: f64, mass: f64, settle_us: u64) -> Spring {
        Spring {
            stiffness,
            damping,
            mass,
            v0: 0.0,
            settle_us,
            rest_delta: 0.5,
            rest_speed: 2.0,
        }
    }

    pub fn velocity(mut self, v0: f64) -> Spring {
        self.v0 = v0;
        self
    }

    pub fn rest(mut self, delta: f64, speed: f64) -> Spring {
        self.rest_delta = delta;
        self.rest_speed = speed;
        self
    }
}

/// How a segment travels from one key to the next.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Ease {
    #[default]
    Linear,
    Cubic {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    Steps {
        count: u32,
        at: StepAt,
    },
    Hold,
    Spring(Spring),
}

impl Ease {
    pub fn cubic(x1: f64, y1: f64, x2: f64, y2: f64) -> Ease {
        Ease::Cubic { x1, y1, x2, y2 }
    }

    pub fn steps(count: u32, at: StepAt) -> Ease {
        Ease::Steps { count, at }
    }

    /// Compile for a segment of `span_us`. The last key never calls this.
    pub fn prepare(self, span_us: u64) -> Result<EasePlan> {
        let kind = match self {
            Ease::Linear => EaseKind::Linear,
            Ease::Hold => EaseKind::Hold,
            Ease::Cubic { x1, y1, x2, y2 } => {
                if !(x1.is_finite() && y1.is_finite() && x2.is_finite() && y2.is_finite()) {
                    return Err(Error::invalid("cubic bezier handles must be finite"));
                }
                if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                    return Err(Error::invalid(format!(
                        "cubic bezier x handles must be in 0 to 1, got {x1} and {x2}"
                    )));
                }
                EaseKind::Bezier(Bezier::new(x1, y1, x2, y2))
            }
            Ease::Steps { count, at } => {
                if count == 0 {
                    return Err(Error::invalid("steps count must be at least 1"));
                }
                if at == StepAt::None && count < 2 {
                    return Err(Error::invalid(
                        "jump-none needs at least 2 steps, because it keeps both endpoints",
                    ));
                }
                EaseKind::Steps { count, at }
            }
            Ease::Spring(spring) => EaseKind::Spring(SpringPlan::prepare(spring, span_us)?),
        };
        let overshoot = match &kind {
            EaseKind::Bezier(bezier) if bezier.y1 > 1.0 || bezier.y2 > 1.0 => {
                max_overshoot(|u| bezier.eval(u))
            }
            EaseKind::Spring(spring) if spring.zeta < 1.0 => max_overshoot(|u| spring.eval(u)),
            _ => 0.0,
        };
        Ok(EasePlan { kind, overshoot })
    }
}

/// A compiled easing. Evaluation is total for any progress.
#[derive(Debug, Clone, PartialEq)]
pub struct EasePlan {
    kind: EaseKind,
    overshoot: f64,
}

impl EasePlan {
    /// Evaluate at progress `u`, clamped to 0 to 1.
    pub fn eval(&self, u: f64) -> f64 {
        let u = if u.is_finite() {
            u.clamp(0.0, 1.0)
        } else {
            0.0
        };
        match &self.kind {
            EaseKind::Linear => u,
            EaseKind::Hold => 0.0,
            EaseKind::Bezier(bezier) => bezier.eval(u),
            EaseKind::Steps { count, at } => steps(u, *count, *at),
            EaseKind::Spring(spring) => spring.eval(u),
        }
    }

    /// The largest amount above 1 that the curve reaches, measured by sampling
    /// it. Zero for curves that cannot overshoot.
    pub fn overshoot(&self) -> f64 {
        self.overshoot
    }

    pub fn is_hold(&self) -> bool {
        matches!(self.kind, EaseKind::Hold)
    }

    /// The residual distance and speed of a spring at its settle time, in
    /// normalised value units. None for any other easing.
    pub fn spring_rest(&self) -> Option<SpringRest> {
        match &self.kind {
            EaseKind::Spring(spring) => Some(spring.rest()),
            _ => None,
        }
    }
}

/// What a spring has left to travel at its settle time, and the thresholds it
/// must meet there. Multiply distance and speed by the segment's travel in
/// value units before comparing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringRest {
    pub distance: f64,
    pub speed: f64,
    pub rest_distance: f64,
    pub rest_speed: f64,
}

#[derive(Debug, Clone, PartialEq)]
enum EaseKind {
    Linear,
    Hold,
    Bezier(Bezier),
    Steps { count: u32, at: StepAt },
    Spring(SpringPlan),
}

/// Cubic bezier with CSS semantics: solved for x, evaluated for y.
#[derive(Debug, Clone, PartialEq)]
struct Bezier {
    y1: f64,
    y2: f64,
    ax: f64,
    bx: f64,
    cx: f64,
    ay: f64,
    by: f64,
    cy: f64,
    samples: [f64; 11],
}

impl Bezier {
    fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Bezier {
        let cx = 3.0 * x1;
        let bx = 3.0 * (x2 - x1) - cx;
        let ax = 1.0 - cx - bx;
        let cy = 3.0 * y1;
        let by = 3.0 * (y2 - y1) - cy;
        let ay = 1.0 - cy - by;
        let mut samples = [0.0; 11];
        for (i, sample) in samples.iter_mut().enumerate() {
            let t = i as f64 / 10.0;
            *sample = ((ax * t + bx) * t + cx) * t;
        }
        Bezier {
            y1,
            y2,
            ax,
            bx,
            cx,
            ay,
            by,
            cy,
            samples,
        }
    }

    fn sample_x(&self, t: f64) -> f64 {
        ((self.ax * t + self.bx) * t + self.cx) * t
    }

    fn sample_dx(&self, t: f64) -> f64 {
        (3.0 * self.ax * t + 2.0 * self.bx) * t + self.cx
    }

    fn sample_y(&self, t: f64) -> f64 {
        ((self.ay * t + self.by) * t + self.cy) * t
    }

    fn eval(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        self.sample_y(self.solve_x(x))
    }

    /// WebKit's UnitBezier: Newton from the table estimate, bisection as guard.
    fn solve_x(&self, x: f64) -> f64 {
        let scaled = x * 10.0;
        let index = (scaled as usize).min(9);
        let x0 = self.samples[index];
        let x1 = self.samples[index + 1];
        let mut t = if x1 > x0 {
            (index as f64 + (x - x0) / (x1 - x0)) / 10.0
        } else {
            (index as f64 + 0.5) / 10.0
        };
        for _ in 0..8 {
            let error = self.sample_x(t) - x;
            if error.abs() < 1e-7 {
                return t;
            }
            let slope = self.sample_dx(t);
            if slope.abs() < 1e-7 {
                break;
            }
            let next = t - error / slope;
            if !(0.0..=1.0).contains(&next) {
                break;
            }
            t = next;
        }
        let mut lo = 0.0;
        let mut hi = 1.0;
        t = t.clamp(0.0, 1.0);
        for _ in 0..64 {
            let error = self.sample_x(t) - x;
            if error.abs() < 1e-10 {
                return t;
            }
            if error < 0.0 {
                lo = t;
            } else {
                hi = t;
            }
            t = 0.5 * (lo + hi);
        }
        t
    }
}

fn steps(u: f64, count: u32, at: StepAt) -> f64 {
    let n = count as f64;
    let value = match at {
        StepAt::Start => ((u * n).floor() + 1.0) / n,
        StepAt::End => (u * n).floor() / n,
        StepAt::None => (u * n).floor() / (n - 1.0),
        StepAt::Both => ((u * n).floor() + 1.0) / (n + 1.0),
    };
    value.clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpringKind {
    Under,
    Critical,
    Over,
}

#[derive(Debug, Clone, PartialEq)]
struct SpringPlan {
    kind: SpringKind,
    omega0: f64,
    zeta: f64,
    omega_d: f64,
    r1: f64,
    r2: f64,
    v0: f64,
    settle_s: f64,
    span_s: f64,
    rest_delta: f64,
    rest_speed: f64,
}

impl SpringPlan {
    fn prepare(spring: Spring, span_us: u64) -> Result<SpringPlan> {
        let values = [
            spring.stiffness,
            spring.damping,
            spring.mass,
            spring.v0,
            spring.rest_delta,
            spring.rest_speed,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err(Error::invalid("spring parameters must be finite"));
        }
        if spring.mass <= 0.0 {
            return Err(Error::invalid("spring mass must be greater than 0"));
        }
        if spring.stiffness <= 0.0 {
            return Err(Error::invalid("spring stiffness must be greater than 0"));
        }
        if spring.damping < 0.0 {
            return Err(Error::invalid("spring damping must be 0 or more"));
        }
        if spring.settle_us == 0 {
            return Err(Error::invalid("spring needs an explicit settle time"));
        }
        if spring.rest_delta < 0.0 || spring.rest_speed < 0.0 {
            return Err(Error::invalid("spring rest thresholds must be 0 or more"));
        }
        if spring.settle_us > span_us {
            return Err(Error::invalid(format!(
                "spring settle of {} us is longer than its segment of {} us",
                spring.settle_us, span_us
            )));
        }
        let omega0 = (spring.stiffness / spring.mass).sqrt();
        let zeta = spring.damping / (2.0 * (spring.stiffness * spring.mass).sqrt());
        let settle_s = spring.settle_us as f64 / 1e6;
        let span_s = span_us as f64 / 1e6;
        let (kind, omega_d, r1, r2) = if zeta < 1.0 - 1e-9 {
            (
                SpringKind::Under,
                omega0 * (1.0 - zeta * zeta).sqrt(),
                0.0,
                0.0,
            )
        } else if zeta > 1.0 + 1e-9 {
            let disc = omega0 * (zeta * zeta - 1.0).sqrt();
            (
                SpringKind::Over,
                0.0,
                -zeta * omega0 + disc,
                -zeta * omega0 - disc,
            )
        } else {
            (SpringKind::Critical, 0.0, 0.0, 0.0)
        };
        Ok(SpringPlan {
            kind,
            omega0,
            zeta,
            omega_d,
            r1,
            r2,
            v0: spring.v0,
            settle_s,
            span_s,
            rest_delta: spring.rest_delta,
            rest_speed: spring.rest_speed,
        })
    }

    fn eval(&self, u: f64) -> f64 {
        if u <= 0.0 {
            return 0.0;
        }
        if u >= 1.0 {
            return 1.0;
        }
        let t = u * self.span_s;
        if t >= self.settle_s {
            return 1.0;
        }
        self.value_at(t)
    }

    /// The unforced curve value at elapsed seconds.
    fn value_at(&self, t: f64) -> f64 {
        match self.kind {
            SpringKind::Under => {
                let decay = libm::exp(-self.zeta * self.omega0 * t);
                let angle = self.omega_d * t;
                let a = (self.zeta * self.omega0 - self.v0) / self.omega_d;
                1.0 - decay * (libm::cos(angle) + a * libm::sin(angle))
            }
            SpringKind::Critical => {
                let decay = libm::exp(-self.omega0 * t);
                1.0 - decay * (1.0 + (self.omega0 - self.v0) * t)
            }
            SpringKind::Over => {
                let c1 = (self.v0 + self.r2) / (self.r2 - self.r1);
                let c2 = 1.0 - c1;
                1.0 - (c1 * libm::exp(self.r1 * t) + c2 * libm::exp(self.r2 * t))
            }
        }
    }

    fn speed_at(&self, t: f64) -> f64 {
        match self.kind {
            SpringKind::Under => {
                let decay = libm::exp(-self.zeta * self.omega0 * t);
                let angle = self.omega_d * t;
                let a = (self.zeta * self.omega0 - self.v0) / self.omega_d;
                decay
                    * ((self.zeta * self.omega0 - a * self.omega_d) * libm::cos(angle)
                        + (self.zeta * self.omega0 * a + self.omega_d) * libm::sin(angle))
            }
            SpringKind::Critical => {
                let decay = libm::exp(-self.omega0 * t);
                let term = (self.omega0 - self.v0) * t;
                decay * (self.omega0 * (1.0 + term) - (self.omega0 - self.v0))
            }
            SpringKind::Over => {
                let c1 = (self.v0 + self.r2) / (self.r2 - self.r1);
                let c2 = 1.0 - c1;
                -(c1 * self.r1 * libm::exp(self.r1 * t) + c2 * self.r2 * libm::exp(self.r2 * t))
            }
        }
    }

    fn rest(&self) -> SpringRest {
        let value = self.value_at(self.settle_s);
        let speed = self.speed_at(self.settle_s);
        SpringRest {
            distance: (1.0 - value).abs(),
            speed: speed.abs(),
            rest_distance: self.rest_delta,
            rest_speed: self.rest_speed,
        }
    }
}

fn max_overshoot(eval: impl Fn(f64) -> f64) -> f64 {
    let mut max: f64 = 1.0;
    for i in 0..=256 {
        let u = i as f64 / 256.0;
        let value = eval(u);
        if value > max {
            max = value;
        }
    }
    (max - 1.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn linear_endpoints_are_exact() {
        let plan = Ease::Linear.prepare(1_000_000).unwrap();
        assert_eq!(plan.eval(0.0), 0.0);
        assert_eq!(plan.eval(1.0), 1.0);
        assert_eq!(plan.eval(0.25), 0.25);
    }

    #[test]
    fn bezier_matches_the_css_reference() {
        // Values from a bisection reference for cubic-bezier(0.25, 0.1, 0.25, 1).
        let plan = Ease::cubic(0.25, 0.1, 0.25, 1.0)
            .prepare(1_000_000)
            .unwrap();
        let table = [
            (0.1, 0.094796306),
            (0.25, 0.408510591),
            (0.5, 0.802403388),
            (0.75, 0.960458978),
            (0.9, 0.994316477),
        ];
        for (x, y) in table {
            assert!(close(plan.eval(x), y, 1e-6), "x {x}: {}", plan.eval(x));
        }
        assert_eq!(plan.eval(0.0), 0.0);
        assert_eq!(plan.eval(1.0), 1.0);
    }

    #[test]
    fn flat_tangent_curve_converges() {
        let plan = Ease::cubic(0.42, 0.0, 1.0, 1.0).prepare(1_000_000).unwrap();
        assert!(close(plan.eval(0.5), 0.315356813, 1e-6));
        assert_eq!(plan.eval(0.0), 0.0);
        assert_eq!(plan.eval(1.0), 1.0);
    }

    #[test]
    fn symmetric_curve_crosses_halfway() {
        let plan = Ease::cubic(0.42, 0.0, 0.58, 1.0)
            .prepare(1_000_000)
            .unwrap();
        assert!(close(plan.eval(0.5), 0.5, 1e-7));
    }

    #[test]
    fn bezier_rejects_x_outside_the_unit_interval() {
        assert!(Ease::cubic(-0.1, 0.0, 1.0, 1.0).prepare(1).is_err());
        assert!(Ease::cubic(0.0, 0.0, 1.1, 1.0).prepare(1).is_err());
        assert!(Ease::cubic(0.0, 2.0, 1.0, -1.0).prepare(1).is_ok());
    }

    #[test]
    fn steps_follow_css_jump_rules() {
        let end = Ease::steps(4, StepAt::End).prepare(1).unwrap();
        assert_eq!(end.eval(0.0), 0.0);
        assert_eq!(end.eval(0.24), 0.0);
        assert_eq!(end.eval(0.25), 0.25);
        assert_eq!(end.eval(0.5), 0.5);
        assert_eq!(end.eval(1.0), 1.0);

        let start = Ease::steps(4, StepAt::Start).prepare(1).unwrap();
        assert_eq!(start.eval(0.0), 0.25);
        assert_eq!(start.eval(0.24), 0.25);
        assert_eq!(start.eval(0.25), 0.5);
        assert_eq!(start.eval(1.0), 1.0);

        let none = Ease::steps(4, StepAt::None).prepare(1).unwrap();
        assert_eq!(none.eval(0.0), 0.0);
        assert!(close(none.eval(1.0 / 3.0), 1.0 / 3.0, 1e-12));
        assert_eq!(none.eval(0.9), 1.0);
        assert_eq!(none.eval(1.0), 1.0);

        let both = Ease::steps(4, StepAt::Both).prepare(1).unwrap();
        assert_eq!(both.eval(0.0), 0.2);
        assert_eq!(both.eval(0.25), 0.4);
        assert_eq!(both.eval(1.0), 1.0);
    }

    #[test]
    fn step_counts_are_validated() {
        assert!(Ease::steps(0, StepAt::End).prepare(1).is_err());
        assert!(Ease::steps(1, StepAt::None).prepare(1).is_err());
        assert!(Ease::steps(1, StepAt::Start).prepare(1).is_ok());
    }

    #[test]
    fn overshoot_is_measured_from_the_curve() {
        let plan = Ease::cubic(0.3, 1.4, 0.7, 1.0).prepare(1_000_000).unwrap();
        assert!(
            close(plan.overshoot(), 0.052892, 1e-4),
            "{}",
            plan.overshoot()
        );
        let plain = Ease::cubic(0.25, 0.1, 0.25, 1.0)
            .prepare(1_000_000)
            .unwrap();
        assert_eq!(plain.overshoot(), 0.0);
    }

    #[test]
    fn spring_reaches_rest_at_its_settle_time() {
        // zeta = 0.5, omega0 = 10 rad/s, one second to settle.
        let plan = Ease::Spring(Spring::new(100.0, 10.0, 1.0, 1_000_000))
            .prepare(1_000_000)
            .unwrap();
        assert_eq!(plan.eval(0.0), 0.0);
        assert_eq!(plan.eval(1.0), 1.0);
        let rest = plan.spring_rest().unwrap();
        assert!(rest.distance < 0.01, "distance {}", rest.distance);
        assert!(rest.speed < 0.5, "speed {}", rest.speed);
        assert!(plan.overshoot() > 0.0);
    }

    #[test]
    fn spring_needs_a_settle_inside_its_segment() {
        let spring = Spring::new(100.0, 10.0, 1.0, 2_000_000);
        assert!(Ease::Spring(spring).prepare(1_000_000).is_err());
        let zero = Spring::new(100.0, 10.0, 1.0, 0);
        assert!(Ease::Spring(zero).prepare(1_000_000).is_err());
    }

    #[test]
    fn spring_parameters_are_validated() {
        assert!(
            Ease::Spring(Spring::new(0.0, 10.0, 1.0, 100))
                .prepare(1_000_000)
                .is_err()
        );
        assert!(
            Ease::Spring(Spring::new(100.0, -1.0, 1.0, 100))
                .prepare(1_000_000)
                .is_err()
        );
        assert!(
            Ease::Spring(Spring::new(100.0, 10.0, 0.0, 100))
                .prepare(1_000_000)
                .is_err()
        );
    }

    #[test]
    fn hold_returns_the_start_value() {
        let plan = Ease::Hold.prepare(1_000_000).unwrap();
        assert_eq!(plan.eval(0.0), 0.0);
        assert_eq!(plan.eval(0.5), 0.0);
        assert_eq!(plan.eval(1.0), 0.0);
        assert!(plan.is_hold());
    }
}
