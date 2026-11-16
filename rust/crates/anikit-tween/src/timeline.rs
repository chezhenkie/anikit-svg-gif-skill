//! Timelines: properties, layers and one prepared plan.
//!
//! `prepare` resolves ids, masks, windows, weights and operation support, then
//! folds a full-weight replace into the property base where it can. `sample`
//! walks the resolved stack with no allocation and no error.

use std::collections::HashSet;

use anikit_core::{Error, Result};

use crate::layer::{AddMode, Binding, Layer, Op, PreparedLayer, PropId, check_weight};
use crate::time::Timing;
use crate::track::{Curve, Lerp, Track};
use crate::value::Value;

/// A property and its base track. Layers contribute through bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub id: PropId,
    pub base: Option<Track<Value>>,
}

impl Property {
    pub fn new(id: PropId, base: Track<Value>) -> Property {
        Property {
            id,
            base: Some(base),
        }
    }

    /// A property that is defined entirely by layers.
    pub fn layered(id: PropId) -> Property {
        Property { id, base: None }
    }
}

/// A duration, a time map, properties and layers.
#[derive(Debug, Clone, PartialEq)]
pub struct Timeline {
    pub duration_us: u64,
    pub timing: Timing,
    pub properties: Vec<Property>,
    pub layers: Vec<Layer>,
}

impl Timeline {
    pub fn new(duration_us: u64, timing: Timing) -> Timeline {
        Timeline {
            duration_us,
            timing,
            properties: Vec::new(),
            layers: Vec::new(),
        }
    }

    pub fn property(mut self, property: Property) -> Timeline {
        self.properties.push(property);
        self
    }

    pub fn properties(mut self, properties: Vec<Property>) -> Timeline {
        self.properties.extend(properties);
        self
    }

    pub fn layer(mut self, layer: Layer) -> Timeline {
        self.layers.push(layer);
        self
    }

    /// Resolve the whole timeline. Fails on an id, order, kind or operation
    /// that cannot produce a defined value.
    pub fn prepare(&self) -> Result<Plan> {
        self.timing.validate()?;
        let mut property_ids = HashSet::new();
        for property in &self.properties {
            if !property_ids.insert(property.id) {
                return Err(Error::invalid(format!(
                    "property {}:{} appears twice",
                    property.id.element, property.id.slot
                )));
            }
        }
        let mut layer_ids = HashSet::new();
        let mut orders = HashSet::new();
        for layer in &self.layers {
            if !layer_ids.insert(layer.id) {
                return Err(Error::invalid(format!(
                    "layer {} appears twice",
                    layer.id.0
                )));
            }
            if !orders.insert(layer.order) {
                return Err(Error::invalid(format!(
                    "layer order {} is used twice",
                    layer.order
                )));
            }
            if let Some(window) = layer.window
                && window.end_us <= window.start_us
            {
                return Err(Error::invalid(format!(
                    "layer {} has a window that ends before it starts",
                    layer.id.0
                )));
            }
        }

        let mut sorted_layers: Vec<&Layer> = self.layers.iter().collect();
        sorted_layers.sort_by_key(|layer| layer.order);

        let mut contributions: Vec<Vec<(&Layer, &Binding)>> =
            vec![Vec::new(); self.properties.len()];
        for layer in &sorted_layers {
            let mut targets = HashSet::new();
            for binding in &layer.bindings {
                let index = self
                    .properties
                    .iter()
                    .position(|property| property.id == binding.target)
                    .ok_or_else(|| {
                        Error::invalid(format!(
                            "layer {} binds to missing property {}:{}",
                            layer.id.0, binding.target.element, binding.target.slot
                        ))
                    })?;
                if !targets.insert(binding.target) {
                    return Err(Error::invalid(format!(
                        "layer {} binds to property {}:{} twice",
                        layer.id.0, binding.target.element, binding.target.slot
                    )));
                }
                contributions[index].push((layer, binding));
            }
        }

        let mut properties = Vec::with_capacity(self.properties.len());
        for (property, contributions) in self.properties.iter().zip(&contributions) {
            properties.push(resolve_property(property, contributions)?);
        }
        Ok(Plan {
            duration_us: self.duration_us,
            timing: self.timing,
            properties,
        })
    }
}

/// A resolved timeline. Sampling maps time once and then reads each property.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    duration_us: u64,
    timing: Timing,
    properties: Vec<PreparedProperty>,
}

impl Plan {
    pub fn duration_us(&self) -> u64 {
        self.duration_us
    }

    pub fn timing(&self) -> Timing {
        self.timing
    }

    pub fn len(&self) -> usize {
        self.properties.len()
    }

    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    pub fn property(&self, index: usize) -> Option<&PreparedProperty> {
        self.properties.get(index)
    }

    /// Map a presentation time onto local time with the plan's timing.
    pub fn local_time(&self, t_us: u64) -> u64 {
        self.timing.map(t_us, self.duration_us)
    }

    /// Sample every property into `out`, stopping at the shorter side.
    pub fn sample_into(&self, t_us: u64, out: &mut [Value]) -> usize {
        let local = self.local_time(t_us);
        let mut count = 0;
        for (slot, property) in out.iter_mut().zip(&self.properties) {
            *slot = property.sample_local(local);
            count += 1;
        }
        count
    }

    /// One property at a presentation time. Allocates a single value, not a scene.
    pub fn property_value(&self, index: usize, t_us: u64) -> Option<Value> {
        let local = self.local_time(t_us);
        self.properties
            .get(index)
            .map(|property| property.sample_local(local))
    }
}

/// One resolved property.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedProperty {
    id: PropId,
    body: Body,
}

impl PreparedProperty {
    pub fn id(&self) -> PropId {
        self.id
    }

    /// True when sampling walks a layer stack instead of one curve.
    pub fn is_stacked(&self) -> bool {
        matches!(self.body, Body::Stacked { .. })
    }

    /// Sample at a local time, after the plan's time map.
    pub fn sample_local(&self, t_us: u64) -> Value {
        let value = match &self.body {
            Body::Single(curve) => curve.sample(t_us),
            Body::Stacked { base, layers } => {
                let mut value = base.sample(t_us);
                for layer in layers {
                    value = layer.sample(t_us, value);
                }
                value
            }
        };
        value.clamped()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Body {
    Single(Curve<Value>),
    Stacked {
        base: Base,
        layers: Vec<PreparedLayer>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum Base {
    Neutral(Value),
    Curve(Curve<Value>),
}

impl Base {
    fn sample(&self, t_us: u64) -> Value {
        match self {
            Base::Neutral(value) => *value,
            Base::Curve(curve) => curve.sample(t_us),
        }
    }
}

struct Candidate<'a> {
    layer: &'a Layer,
    binding: &'a Binding,
    value: Curve<Value>,
    weight: Curve<f64>,
    constant_weight: Option<f64>,
}

fn resolve_property(
    property: &Property,
    contributions: &[(&Layer, &Binding)],
) -> Result<PreparedProperty> {
    let name = format!("property {}:{}", property.id.element, property.id.slot);
    let kind = if let Some(base) = &property.base {
        base.keys
            .first()
            .map(|key| key.value)
            .ok_or_else(|| Error::invalid(format!("{name}: the base track has no keys")))?
    } else if let Some((_, binding)) = contributions.first() {
        binding
            .track
            .keys
            .first()
            .map(|key| key.value)
            .ok_or_else(|| Error::invalid(format!("{name}: a binding track has no keys")))?
    } else {
        return Err(Error::invalid(format!(
            "{name}: no base track and no binding"
        )));
    };

    let base_curve = match &property.base {
        Some(track) => {
            check_kind(track, &kind, &name)?;
            Some(track.prepare_named(&name)?)
        }
        None => None,
    };

    let mut candidates = Vec::with_capacity(contributions.len());
    for (layer, binding) in contributions {
        if layer.mask.is_some() {
            return Err(Error::invalid(format!(
                "{name}: a mask cannot apply to a {} property",
                kind.kind_name()
            )));
        }
        check_kind(&binding.track, &kind, &name)?;
        check_op(
            binding.op,
            binding.mode,
            binding.allow_color_add,
            &kind,
            &name,
        )?;
        let weight_track = binding.weight.as_ref().unwrap_or(&layer.weight);
        check_weight(
            weight_track,
            &format!("{name} layer {}", layer.id.0),
            binding.overshoot,
        )?;
        let constant_weight = if weight_track.keys.len() == 1 {
            Some(weight_track.keys[0].value)
        } else {
            None
        };
        let weight = weight_track.prepare_named(&format!("{name} weight"))?;
        let value = binding
            .track
            .prepare_named(&format!("{name} layer {}", layer.id.0))?;
        candidates.push(Candidate {
            layer,
            binding,
            value,
            weight,
            constant_weight,
        });
    }

    if matches!(kind, Value::Transform(_))
        && candidates
            .iter()
            .any(|candidate| matches!(candidate.binding.op, Op::Add | Op::Accumulate))
    {
        if let Some(base) = &property.base {
            check_add_safe(base, &name)?;
        }
        for candidate in &candidates {
            if matches!(candidate.binding.op, Op::Add | Op::Accumulate) {
                check_add_safe(&candidate.binding.track, &name)?;
            }
        }
    }

    let active: Vec<Candidate> = candidates
        .into_iter()
        .filter(|candidate| candidate.constant_weight != Some(0.0))
        .collect();
    if active.is_empty() {
        return match base_curve {
            Some(curve) => Ok(PreparedProperty {
                id: property.id,
                body: Body::Single(curve),
            }),
            None => Err(Error::invalid(format!(
                "{name}: every binding has zero weight and there is no base"
            ))),
        };
    }

    // A full-weight replace with no window hides everything below it, so it
    // becomes the base and the layers before it need not be sampled.
    let folded = active.iter().rposition(|candidate| {
        candidate.binding.op == Op::Replace
            && candidate.constant_weight == Some(1.0)
            && candidate.layer.window.is_none()
    });
    let (base, start) = if let Some(index) = folded {
        (Base::Curve(active[index].value.clone()), index + 1)
    } else if let Some(curve) = base_curve {
        (Base::Curve(curve), 0)
    } else {
        (Base::Neutral(kind.neutral_like()), 0)
    };
    let layers: Vec<PreparedLayer> = active[start..]
        .iter()
        .map(|candidate| PreparedLayer {
            value: candidate.value.clone(),
            weight: candidate.weight.clone(),
            op: candidate.binding.op,
            mode: candidate.binding.mode,
            window: candidate.layer.window,
            clamp_weight: !candidate.binding.overshoot,
        })
        .collect();
    if layers.is_empty() {
        let Base::Curve(curve) = base else {
            return Err(Error::invalid(format!(
                "{name}: no contributing value and no base"
            )));
        };
        return Ok(PreparedProperty {
            id: property.id,
            body: Body::Single(curve),
        });
    }
    Ok(PreparedProperty {
        id: property.id,
        body: Body::Stacked { base, layers },
    })
}

fn check_kind(track: &Track<Value>, kind: &Value, name: &str) -> Result<()> {
    for key in &track.keys {
        if !key.value.same_kind(kind) {
            return Err(Error::invalid(format!(
                "{name}: a {} binding cannot combine with a {} property",
                key.value.kind_name(),
                kind.kind_name()
            )));
        }
    }
    Ok(())
}

fn check_op(op: Op, mode: AddMode, allow_color_add: bool, kind: &Value, name: &str) -> Result<()> {
    if !matches!(kind, Value::Transform(_)) && mode != AddMode::Append {
        return Err(Error::invalid(format!(
            "{name}: add mode applies to transform properties only"
        )));
    }
    match (op, kind) {
        (Op::Replace, _) => Ok(()),
        (_, Value::Number(_)) | (_, Value::Opacity(_)) | (_, Value::Transform(_)) => Ok(()),
        (Op::Multiply, Value::Color(_)) => Ok(()),
        (Op::Add, Value::Color(_)) if allow_color_add => Ok(()),
        (Op::Add, Value::Color(_)) => Err(Error::invalid(format!(
            "{name}: colour is not additive without explicit permission"
        ))),
        (Op::Accumulate, Value::Color(_)) => {
            Err(Error::invalid(format!("{name}: colour cannot accumulate")))
        }
    }
}

fn check_add_safe(track: &Track<Value>, name: &str) -> Result<()> {
    for key in &track.keys {
        if let Some(transform) = key.value.as_transform()
            && !transform.is_add_safe()
        {
            return Err(Error::invalid(format!(
                "{name}: add meets a scale or skew at {}",
                key.t_us
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{LayerId, Mask, Op, Window};
    use crate::track::Key;
    use crate::value::{Color, Transform};
    use std::f64::consts::FRAC_PI_2;

    const D: u64 = 1_000_000;

    fn prop(slot: u16, value: Value) -> Property {
        Property::new(PropId::new(0, slot), Track::constant(value))
    }

    fn layer(order: u32, weight: f64, binding: Binding) -> Layer {
        Layer::new(LayerId(order), order, Track::constant(weight)).binding(binding)
    }

    #[test]
    fn weight_zero_is_identity_for_every_type_and_operation() {
        fn check(base: Value, layer: Value, ops: &[Op]) {
            for op in ops {
                let target = PropId::new(0, 0);
                let mut binding = Binding::new(target, *op, Track::constant(layer));
                if matches!(base, Value::Color(_)) && *op == Op::Add {
                    binding = binding.allow_color_add();
                }
                let timeline = Timeline::new(D, Timing::default())
                    .property(Property::new(target, Track::constant(base)))
                    .layer(Layer::new(LayerId(1), 1, Track::constant(0.0)).binding(binding));
                let plan = timeline.prepare().unwrap();
                assert_eq!(plan.property_value(0, 0), Some(base), "{op:?} on {base:?}");
            }
        }
        check(
            Value::Number(5.0),
            Value::Number(9.0),
            &[Op::Replace, Op::Add, Op::Multiply, Op::Accumulate],
        );
        check(
            Value::Opacity(0.5),
            Value::Opacity(0.4),
            &[Op::Replace, Op::Add, Op::Multiply, Op::Accumulate],
        );
        check(
            Value::Transform(Transform::translate(3.0, 4.0)),
            Value::Transform(Transform::translate(9.0, 9.0)),
            &[Op::Replace, Op::Add, Op::Multiply, Op::Accumulate],
        );
        check(
            Value::Color(Color::linear(0.2, 0.4, 0.6, 1.0)),
            Value::Color(Color::linear(1.0, 0.0, 0.0, 1.0)),
            &[Op::Replace, Op::Add, Op::Multiply],
        );
    }

    #[test]
    fn three_add_layers_sum_in_any_array_order() {
        fn build(reversed: bool) -> Timeline {
            let mut layers = Vec::new();
            for order in 1..=3 {
                layers.push(layer(
                    order,
                    1.0,
                    Binding::new(
                        PropId::new(0, 0),
                        Op::Add,
                        Track::constant(Value::Number(0.1)),
                    ),
                ));
            }
            if reversed {
                layers.reverse();
            }
            let mut timeline =
                Timeline::new(D, Timing::default()).property(prop(0, Value::Number(0.2)));
            for layer in layers {
                timeline = timeline.layer(layer);
            }
            timeline
        }
        for reversed in [false, true] {
            let plan = build(reversed).prepare().unwrap();
            let value = plan.property_value(0, 0).unwrap();
            let Value::Number(value) = value else {
                panic!("expected a number");
            };
            assert!((value - 0.5).abs() < 1e-15, "{value}");
        }
    }

    #[test]
    fn transform_append_and_prepend_differ() {
        fn build(prepend: bool, swapped: bool) -> Transform {
            let target = PropId::new(0, 0);
            let mut rotate = Binding::new(
                target,
                Op::Add,
                Track::constant(Value::Transform(Transform::rotate(FRAC_PI_2))),
            );
            let mut translate = Binding::new(
                target,
                Op::Add,
                Track::constant(Value::Transform(Transform::translate(20.0, 0.0))),
            );
            if prepend {
                rotate = rotate.prepend();
                translate = translate.prepend();
            }
            let (rotate_order, translate_order) = if swapped { (2, 1) } else { (1, 2) };
            let timeline = Timeline::new(D, Timing::default())
                .property(Property::new(
                    target,
                    Track::constant(Value::Transform(Transform::translate(10.0, 0.0))),
                ))
                .layer(Layer::new(LayerId(1), rotate_order, Track::constant(1.0)).binding(rotate))
                .layer(
                    Layer::new(LayerId(2), translate_order, Track::constant(1.0))
                        .binding(translate),
                );
            let plan = timeline.prepare().unwrap();
            let Value::Transform(transform) = plan.property_value(0, 0).unwrap() else {
                panic!("expected a transform");
            };
            transform
        }
        let append = build(false, false);
        let prepend = build(true, false);
        let swapped_append = build(false, true);
        let swapped_prepend = build(true, true);
        let close =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
        assert!(close(append.apply(0.0, 0.0), [10.0, 20.0]));
        assert!(close(prepend.apply(0.0, 0.0), [20.0, 10.0]));
        assert!(close(swapped_append.apply(0.0, 0.0), [30.0, 0.0]));
        assert!(close(swapped_prepend.apply(0.0, 0.0), [0.0, 30.0]));
    }

    #[test]
    fn opacity_is_clamped_once_after_the_stack() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Opacity(1.5)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Opacity(0.5)),
                ),
            ))
            .layer(layer(
                2,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Multiply,
                    Track::constant(Value::Opacity(0.5)),
                ),
            ));
        let plan = timeline.prepare().unwrap();
        assert_eq!(plan.property_value(0, 0), Some(Value::Opacity(1.0)));
    }

    #[test]
    fn duplicate_layer_order_is_rejected() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(0.0)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Number(1.0)),
                ),
            ))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Number(1.0)),
                ),
            ));
        assert!(timeline.prepare().is_err());
    }

    #[test]
    fn a_binding_to_a_missing_property_is_rejected() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(0.0)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(9, 0),
                    Op::Add,
                    Track::constant(Value::Number(1.0)),
                ),
            ));
        assert!(timeline.prepare().is_err());
    }

    #[test]
    fn colour_add_needs_explicit_permission() {
        fn build(allow: bool) -> Result<Plan> {
            let mut binding = Binding::new(
                PropId::new(0, 0),
                Op::Add,
                Track::constant(Value::Color(Color::linear(1.0, 0.0, 0.0, 1.0))),
            );
            if allow {
                binding = binding.allow_color_add();
            }
            Timeline::new(D, Timing::default())
                .property(prop(0, Value::Color(Color::linear(0.0, 0.0, 0.0, 1.0))))
                .layer(layer(1, 1.0, binding))
                .prepare()
        }
        assert!(build(false).is_err());
        let plan = build(true).unwrap();
        let Some(Value::Color(colour)) = plan.property_value(0, 0) else {
            panic!("expected a colour");
        };
        assert!((colour.r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_mask_on_a_transform_is_rejected() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Transform(Transform::IDENTITY)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Transform(Transform::translate(1.0, 0.0))),
                ),
            ))
            .layer(
                Layer::new(LayerId(2), 2, Track::constant(1.0))
                    .mask(Mask::Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 10.0,
                        h: 10.0,
                    })
                    .binding(Binding::new(
                        PropId::new(0, 0),
                        Op::Add,
                        Track::constant(Value::Transform(Transform::translate(1.0, 0.0))),
                    )),
            );
        let error = timeline.prepare().unwrap_err().to_string();
        assert!(error.contains("mask"), "{error}");
    }

    #[test]
    fn a_full_weight_replace_folds_into_a_single_curve() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(1.0)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Replace,
                    Track::constant(Value::Number(2.0)),
                ),
            ));
        let plan = timeline.prepare().unwrap();
        assert!(!plan.property(0).unwrap().is_stacked());
        assert_eq!(plan.property_value(0, 0), Some(Value::Number(2.0)));
    }

    #[test]
    fn replace_weight_fades_between_base_and_layer() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(10.0)))
            .layer(layer(
                1,
                0.5,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Replace,
                    Track::constant(Value::Number(0.0)),
                ),
            ));
        let plan = timeline.prepare().unwrap();
        assert_eq!(plan.property_value(0, 0), Some(Value::Number(5.0)));
    }

    #[test]
    fn weights_above_one_need_overshoot() {
        fn build(overshoot: bool) -> Result<Plan> {
            let mut binding = Binding::new(
                PropId::new(0, 0),
                Op::Add,
                Track::constant(Value::Number(2.0)),
            );
            if overshoot {
                binding = binding.overshoot();
            }
            Timeline::new(D, Timing::default())
                .property(prop(0, Value::Number(1.0)))
                .layer(layer(1, 1.5, binding))
                .prepare()
        }
        assert!(build(false).is_err());
        assert_eq!(
            build(true).unwrap().property_value(0, 0),
            Some(Value::Number(4.0))
        );
    }

    #[test]
    fn a_layer_window_switches_the_effect_off() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(0.0)))
            .layer(
                Layer::new(LayerId(1), 1, Track::constant(1.0))
                    .window(Window::new(200_000, 400_000))
                    .binding(Binding::new(
                        PropId::new(0, 0),
                        Op::Add,
                        Track::constant(Value::Number(5.0)),
                    )),
            );
        let plan = timeline.prepare().unwrap();
        assert_eq!(plan.property_value(0, 100_000), Some(Value::Number(0.0)));
        assert_eq!(plan.property_value(0, 300_000), Some(Value::Number(5.0)));
        assert_eq!(plan.property_value(0, 400_000), Some(Value::Number(0.0)));
    }

    #[test]
    fn the_plan_applies_cycle_and_rate() {
        let base = Track::new(vec![
            Key::new(0, Value::Number(0.0)),
            Key::new(D, Value::Number(10.0)),
        ]);
        let timeline = Timeline::new(D, Timing::new(crate::LoopMode::Cycle, 1.0))
            .property(Property::new(PropId::new(0, 0), base));
        let plan = timeline.prepare().unwrap();
        assert_eq!(plan.property_value(0, 1_250_000), Some(Value::Number(2.5)));
        assert_eq!(plan.property_value(0, 250_000), Some(Value::Number(2.5)));
    }

    #[test]
    fn a_layered_property_without_a_base_starts_from_neutral() {
        let timeline = Timeline::new(D, Timing::default())
            .property(Property::layered(PropId::new(0, 0)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Number(3.0)),
                ),
            ));
        let plan = timeline.prepare().unwrap();
        assert_eq!(plan.property_value(0, 0), Some(Value::Number(3.0)));
    }

    #[test]
    fn a_scaled_transform_cannot_be_added_to() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Transform(Transform::scale(2.0, 2.0))))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Transform(Transform::translate(1.0, 0.0))),
                ),
            ));
        assert!(timeline.prepare().is_err());
    }

    #[test]
    fn add_mode_is_transform_only() {
        let timeline = Timeline::new(D, Timing::default())
            .property(prop(0, Value::Number(0.0)))
            .layer(layer(
                1,
                1.0,
                Binding::new(
                    PropId::new(0, 0),
                    Op::Add,
                    Track::constant(Value::Number(1.0)),
                )
                .prepend(),
            ));
        assert!(timeline.prepare().is_err());
    }
}
