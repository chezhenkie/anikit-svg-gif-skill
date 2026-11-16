//! Keyframes, easing, property values and layers.
//!
//! Values over time only. This crate knows nothing about pixels, files or
//! fonts, and it must not allocate inside a sample call.
//!
//! The rules are docs/specsheet/04-tween-model.md through 07-tween-composition.md.
//! Time is integer microseconds; `prepare` compiles a track and may fail, while
//! `sample` is total and allocation free.

mod ease;
mod layer;
mod time;
mod timeline;
mod track;
mod value;

pub use ease::{Ease, EasePlan, Spring, SpringRest, StepAt};
pub use layer::{AddMode, Binding, Layer, LayerId, Mask, Op, PropId, Window};
pub use time::{LoopMode, Timing};
pub use timeline::{Plan, PreparedProperty, Property, Timeline};
pub use track::{Ballistic, Curve, Key, Lerp, Motion, Smooth, Track};
pub use value::{Color, Decomposed, Transform, Value};
