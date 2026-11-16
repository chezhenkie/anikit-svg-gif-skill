//! Sampling a prepared plan must not allocate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use anikit_tween::{
    Binding, Color, Ease, Key, Layer, LayerId, LoopMode, Op, Plan, PropId, Property, Timeline,
    Timing, Track, Transform, Value,
};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn number_track() -> Track<Value> {
    Track::new(vec![
        Key::new(0, Value::Number(0.0)),
        Key::new(500_000, Value::Number(4.0)),
        Key::new(1_000_000, Value::Number(10.0)),
    ])
}

fn build_plan() -> Plan {
    let target = PropId::new(0, 0);
    let weight = Track::new(vec![
        Key::new(0, 0.2).ease(Ease::cubic(0.25, 0.1, 0.25, 1.0)),
        Key::new(500_000, 0.8),
        Key::new(1_000_000, 0.2),
    ]);
    let layer = Layer::new(LayerId(1), 1, weight)
        .binding(Binding::new(
            PropId::new(0, 0),
            Op::Add,
            Track::constant(Value::Number(5.0)),
        ))
        .binding(Binding::new(
            PropId::new(0, 1),
            Op::Multiply,
            Track::constant(Value::Opacity(0.5)),
        ))
        .binding(Binding::new(
            PropId::new(0, 2),
            Op::Add,
            Track::constant(Value::Transform(Transform::translate(3.0, 2.0))),
        ))
        .binding(Binding::new(
            PropId::new(0, 3),
            Op::Multiply,
            Track::constant(Value::Color(Color::linear(0.5, 0.5, 0.5, 1.0))),
        ));
    let replace = Layer::new(LayerId(2), 2, Track::constant(0.5)).binding(Binding::new(
        target,
        Op::Replace,
        Track::constant(Value::Number(20.0)),
    ));
    Timeline::new(1_000_000, Timing::new(LoopMode::Cycle, 1.0))
        .property(Property::new(target, number_track()))
        .property(Property::new(
            PropId::new(0, 1),
            Track::constant(Value::Opacity(1.0)),
        ))
        .property(Property::new(
            PropId::new(0, 2),
            Track::constant(Value::Transform(Transform::translate(1.0, 0.0))),
        ))
        .property(Property::new(
            PropId::new(0, 3),
            Track::constant(Value::Color(Color::linear(1.0, 0.5, 0.25, 1.0))),
        ))
        .layer(layer)
        .layer(replace)
        .prepare()
        .unwrap()
}

#[test]
fn sampling_a_plan_allocates_nothing() {
    let plan = build_plan();
    let mut out = vec![Value::Number(0.0); plan.len()];
    plan.sample_into(0, &mut out);
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    for frame in 0..1000u64 {
        plan.sample_into(frame * 1000, &mut out);
    }
    let after = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(after, before, "sampling allocated {} times", after - before);
}
