//! Layers: order, weight, windows and the per-type combine rules.
//!
//! A layer contributes bindings to properties. Prepare resolves the stack; a
//! sample walks it, skipping closed windows and zero weights, and mixes only
//! when the weight is strictly between 0 and 1.

use anikit_core::{Error, Result};

use crate::track::{Curve, Lerp, Track};
use crate::value::{Color, Value};

/// A stable layer identifier, used in messages and baked exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u32);

/// A property inside an element: the element id and a slot number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropId {
    pub element: u32,
    pub slot: u16,
}

impl PropId {
    pub fn new(element: u32, slot: u16) -> PropId {
        PropId { element, slot }
    }
}

/// A hard time mask in local microseconds, half open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start_us: u64,
    pub end_us: u64,
}

impl Window {
    pub fn new(start_us: u64, end_us: u64) -> Window {
        Window { start_us, end_us }
    }

    pub fn is_open(&self, t_us: u64) -> bool {
        self.start_us <= t_us && t_us < self.end_us
    }
}

/// How a binding combines with the value beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Replace,
    Add,
    Multiply,
    Accumulate,
}

/// Where a transform layer sits relative to the base axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AddMode {
    #[default]
    Append,
    Prepend,
}

/// A spatial mask. Only per-pixel types can carry one; the scalar stack rejects
/// it because moving half an element is a renderer operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mask {
    Rect { x: f64, y: f64, w: f64, h: f64 },
    Cells { start: u32, end: u32 },
    Alpha(u32),
}

/// One layer's contribution to one property.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub target: PropId,
    pub op: Op,
    pub track: Track<Value>,
    pub weight: Option<Track<f64>>,
    pub mode: AddMode,
    pub overshoot: bool,
    pub allow_color_add: bool,
}

impl Binding {
    pub fn new(target: PropId, op: Op, track: Track<Value>) -> Binding {
        Binding {
            target,
            op,
            track,
            weight: None,
            mode: AddMode::Append,
            overshoot: false,
            allow_color_add: false,
        }
    }

    pub fn weight(mut self, weight: Track<f64>) -> Binding {
        self.weight = Some(weight);
        self
    }

    pub fn prepend(mut self) -> Binding {
        self.mode = AddMode::Prepend;
        self
    }

    pub fn overshoot(mut self) -> Binding {
        self.overshoot = true;
        self
    }

    pub fn allow_color_add(mut self) -> Binding {
        self.allow_color_add = true;
        self
    }
}

/// A named stack of bindings with a default weight and a hard window.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub order: u32,
    pub window: Option<Window>,
    pub weight: Track<f64>,
    pub mask: Option<Mask>,
    pub bindings: Vec<Binding>,
}

impl Layer {
    pub fn new(id: LayerId, order: u32, weight: Track<f64>) -> Layer {
        Layer {
            id,
            order,
            window: None,
            weight,
            mask: None,
            bindings: Vec::new(),
        }
    }

    pub fn window(mut self, window: Window) -> Layer {
        self.window = Some(window);
        self
    }

    pub fn mask(mut self, mask: Mask) -> Layer {
        self.mask = Some(mask);
        self
    }

    pub fn binding(mut self, binding: Binding) -> Layer {
        self.bindings.push(binding);
        self
    }
}

/// A compiled layer. The value track is sampled only after the window is open
/// and the weight is not zero.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PreparedLayer {
    pub value: Curve<Value>,
    pub weight: Curve<f64>,
    pub op: Op,
    pub mode: AddMode,
    pub window: Option<Window>,
    pub clamp_weight: bool,
}

impl PreparedLayer {
    pub fn sample(&self, t_us: u64, base: Value) -> Value {
        if let Some(window) = self.window
            && !window.is_open(t_us)
        {
            return base;
        }
        let mut weight = self.weight.sample(t_us);
        if self.clamp_weight {
            weight = weight.clamp(0.0, 1.0);
        }
        if weight == 0.0 {
            return base;
        }
        let value = self.value.sample(t_us);
        let combined = combine(base, value, self.op, self.mode);
        if weight == 1.0 {
            combined
        } else {
            base.mix(&combined, weight)
        }
    }
}

/// Weight keys must be in 0 to 1 unless the binding declares overshoot.
pub(crate) fn check_weight(track: &Track<f64>, name: &str, overshoot: bool) -> Result<()> {
    for key in &track.keys {
        if !key.value.is_finite() {
            return Err(Error::invalid(format!(
                "{name}: weight at {} is not finite",
                key.t_us
            )));
        }
        if !overshoot && !(0.0..=1.0).contains(&key.value) {
            return Err(Error::invalid(format!(
                "{name}: weight at {} is outside 0 to 1 and overshoot is not allowed",
                key.t_us
            )));
        }
    }
    Ok(())
}

/// `v = mix(v, combine(v, x), w)`, exact at both ends.
pub(crate) fn combine(base: Value, layer: Value, op: Op, mode: AddMode) -> Value {
    match op {
        Op::Replace => layer,
        Op::Add | Op::Accumulate => match (base, layer) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a + b),
            (Value::Opacity(a), Value::Opacity(b)) => Value::Opacity(a + b),
            (Value::Transform(a), Value::Transform(b)) => {
                let value = if op == Op::Accumulate {
                    a.accumulate(&b)
                } else if mode == AddMode::Prepend {
                    b.mul(&a)
                } else {
                    a.mul(&b)
                };
                Value::Transform(value)
            }
            (Value::Color(a), Value::Color(b)) => Value::Color(Color {
                r: a.r + b.r,
                g: a.g + b.g,
                b: a.b + b.b,
                a: a.a,
            }),
            (base, _) => base,
        },
        Op::Multiply => match (base, layer) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a * b),
            (Value::Opacity(a), Value::Opacity(b)) => Value::Opacity(a * b),
            (Value::Transform(a), Value::Transform(b)) => {
                Value::Transform(if mode == AddMode::Prepend {
                    b.mul(&a)
                } else {
                    a.mul(&b)
                })
            }
            (Value::Color(a), Value::Color(b)) => Value::Color(Color {
                r: a.r * b.r,
                g: a.g * b.g,
                b: a.b * b.b,
                a: a.a * b.a,
            }),
            (base, _) => base,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Transform;

    #[test]
    fn colour_add_keeps_the_base_alpha() {
        let base = Value::Color(Color::linear(0.2, 0.2, 0.2, 0.5));
        let layer = Value::Color(Color::linear(0.4, 0.0, 0.0, 0.5));
        let Value::Color(out) = combine(base, layer, Op::Add, AddMode::Append) else {
            panic!("expected a colour");
        };
        assert!((out.r - 0.3).abs() < 1e-6, "{}", out.r);
        assert_eq!(out.a, 0.5);
    }

    #[test]
    fn append_and_prepend_put_a_rotation_in_different_frames() {
        let base = Value::Transform(Transform::translate(10.0, 0.0));
        let layer = Value::Transform(Transform::rotate(std::f64::consts::FRAC_PI_2));
        let Value::Transform(append) = combine(base, layer, Op::Add, AddMode::Append) else {
            panic!("expected a transform");
        };
        let Value::Transform(prepend) = combine(base, layer, Op::Add, AddMode::Prepend) else {
            panic!("expected a transform");
        };
        let a = append.apply(0.0, 0.0);
        let p = prepend.apply(0.0, 0.0);
        assert!((a[0] - 10.0).abs() < 1e-9 && a[1].abs() < 1e-9, "{a:?}");
        assert!(p[0].abs() < 1e-9 && (p[1] - 10.0).abs() < 1e-9, "{p:?}");
    }

    #[test]
    fn weights_clamp_unless_overshoot_is_allowed() {
        let track = Track::new(vec![crate::Key::new(0, 1.5)]);
        assert!(check_weight(&track, "layer", false).is_err());
        assert!(check_weight(&track, "layer", true).is_ok());
    }
}
