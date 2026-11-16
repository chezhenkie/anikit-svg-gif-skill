# 04-tween-model

Status: implemented. Evidence and comparisons: docs/research/tween-timeline-model.md. Open rows are cited as 09, row n.

## 4.1 Scope

The crate turns a validated timeline into one value per animated property at a requested time. It knows nothing about pixels, files, fonts or the browser. It does not decide how many frames exist: that is the frame plan in `time`.

Two calls exist. `prepare` takes the input and either fails or returns a plan. `sample` takes a plan and a time and returns values, and it is total: every time in the range produces a value, with no error and no allocation.

## 4.2 Time

Time is integer microseconds in a `u64`. The conversion to `f64` happens once per sample, at the top, and no float time is stored.

```rust
pub struct Timing { pub loop_mode: LoopMode, pub rate: f64 }
pub enum LoopMode { Once, Cycle, PingPong }
```

- `rate` and `loop_mode` are applied by a time map before `sample`, so `sample(t)` stays a pure function of `t` in `0..duration`. The rate is finite and `|rate| * duration` must stay inside the float range, so a mapped time is always a real number of microseconds; the error names `timing.rate` and the largest rate the scene's duration allows.
- Ping-pong is the triangle map over the duration; reverse playback is `rate = -1`; neither is data in a track.
- The frame plan owns the frame count and the presentation times. A plan that carries the loop end can drop a repeated seam frame with `FramePlan::without_last`; the tween route's count stops one frame before the end, so it has no duplicate to drop.

## 4.3 Tracks and keys

A track is an ordered list of keys, each with an absolute time and a value. A key may carry the easing of the segment it starts, which is the outgoing segment.

- Keys are sorted at prepare. Unsorted input is an error, not something to repair silently.
- Two keys may share a time only when the earlier one holds. That is a step, and a lookup takes the last key at or before the time.
- Outside the key range the value clamps to the first or last key.
- A track with one key is constant over the whole duration.
- A zero-length segment is a jump to the later key.

## 4.4 Easing ownership

Easing belongs to the key the segment leaves. This is the WAAPI and Lottie convention and the one the crate stores.

- The last key's easing is never used, because it starts no segment.
- `hold` on a key means the segment after it does not interpolate; the value stays until the next key.
- SMIL is the outlier: `keySplines` is indexed by interval and must be shifted by one on import, so interval `i` becomes the easing of key `i`.
- Easing is per track, not per property inside a keyframe offset. A format that allows a different easing per property is split into tracks on import.

## 4.5 Repetition

Repetition is not part of the data. A repeated animation is the same timeline sampled through a different time map, so `sample` never counts iterations and never holds a cursor.

| mode | time map |
|---|---|
| once | `t` as given, clamped |
| cycle | `t mod duration` |
| pingpong | triangle over `duration`, then clamped |

Play count is a property of the frame plan, not of the timeline. A plan with `count = 3` emits three passes of presentation times.

## 4.6 Canonical input

One file, absolute times in milliseconds, keys in order, easing on the outgoing key. This is the shape the CLI accepts and the shape every import is normalised to.

```json
{
  "version": 1,
  "canvas": {"w": 480, "h": 270, "bg": "#0a0d12", "fps": 25},
  "timing": {"loop": "cycle", "rate": 1.0},
  "tracks": [
    {"id": "logo", "kind": "svg.transform", "src": "keys/logo.svg#mark",
     "keys": [
       {"t": 0, "translate": [0, 0], "easing": "ease-out"},
       {"t": 800, "translate": [120, 40], "easing": "hold"},
       {"t": 1400, "translate": [120, 40]}
     ]}
  ],
  "layers": []
}
```

`kind` selects the property type in part 06. `src` uses `file.svg#elementId`. A missing attribute inherits the previous key. `layers` is defined in part 07. Part 11 fixes the full grammar the tween route accepts, including the five kinds it draws, the easing and motion objects, and the layer syntax.

## 4.7 Invariants and tests

- `sample` at a key time returns that key's value exactly, with no float drift.
- `sample` before the first key and after the last key returns the clamped endpoint.
- A `hold` key keeps its value until the next key, and the value changes in one step.
- A reverse time map applied twice returns the original sequence of values.
- A cycle plan contains no frame identical to its successor at the seam unless the animation holds there.
- `sample` allocates nothing after `prepare`.
- Two runs on different platforms produce identical values; the test pins this with fixed inputs and a fixed libm.
