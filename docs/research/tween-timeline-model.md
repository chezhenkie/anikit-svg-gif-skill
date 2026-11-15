# Tween timeline model

Research notes on the time semantics a Rust frame sampler needs before it can read Lottie, CSS/WAAPI, SMIL, GSAP or Rive. Not a specification.

## Scope

The crate evaluates an animation at a time `t` and returns one value per track. This note covers the time and easing semantics of five notations, one model that contains them, and the tests that pin each rule. Rendering, path correspondence, colour spaces, compositing and encoding are outside it. Times are `f64` milliseconds, `t = 0` is the first frame, a segment covers `[key.t, next.t)`, and the last key is a point.

`docs/keyframe-tween-animation-info.md` section 2 fixes absolute times and easing on the outgoing segment. This note adds what that sketch leaves open: tangent ownership, duplicate times, loop mapping and nested time.

## Comparison table

| Format | Easing ownership | Hold keys | Tangents | Composite modes | Loop | Source |
|---|---|---|---|---|---|---|
| Lottie | On the key that starts the segment: `o` and `i` are the two cubic handles, so `cubic-bezier(o.x, o.y, i.x, i.y)`; optionally per component. | `h: 1` holds to the next key; handles unused. | `to` and `ti` on the start key, value units: `s_k, s_k+to, s_{k+1}+ti, s_{k+1}`; the eased parameter is arc length. | None. | None in the file; `ip`/`op` bound it. | `PropertyFactory.js`, lottie-spec |
| CSS animations | Keyframe `animation-timing-function`, applied to the next keyframe; ignored on `to`. | `steps(1, jump-end)`. | None. | `animation-composition` replace, add, accumulate; sticky per property. | Iteration count plus direction (`alternate`). | css-animations-1 3.1, 2 4.10 |
| WAAPI | Per-keyframe `easing`, from this key to the next; never applied on the last. | `steps(1, jump-end)`. | None. | Per-key `composite`, effect `composite`, `iterationComposite`. | Iterations, direction, iteration start. | web-animations-1 5.3.1, 5.4.4 |
| SMIL | Per interval: `keySplines[i]` sits between value `i` and value `i+1`. | `calcMode="discrete"`, whole animation. | None; `animateMotion` carries a path. | `additive="sum"`, `accumulate="sum"`, per animation. | Repeat count and duration; no ping-pong. | SMIL Animation 3.2.3 |
| GSAP | Per tween; one tween is one segment. | `ease: "steps(1)"`, zero-duration tween. | None. | None. | Repeat, yoyo, nested timelines. | Position parameter, `gsap-core.js` |
| Rive | On the first key of a pair. | The `Hold` interpolation type. | Cubic handles in value space. | None per key; state machines blend animations. | One-Shot, Loop, Ping-Pong; negative speed. | Rive docs |

## Recommended Rust data model

### Easing ownership

Every notation here except SMIL puts easing on the outgoing key; SMIL indexes its splines by interval instead. The model follows the majority: a key stays self-contained when a neighbour is added, and a missing curve has one meaning. The last key has no outgoing segment, so its `ease` is dead: the loader drops it. A hold never evaluates its curve, so `hold` wins over `ease` and `blend`.

The loop seam is not a segment, so no key owns a seam curve. Under `cycle` the value jumps unless the end and start values agree, and the velocity jumps unless the slopes agree. Under `pingpong` time mirrors, so the value is continuous and the velocity is negated, smooth only at a zero endpoint slope.

Reverse playback traverses each curve backwards, so an ease-out reads as an ease-in. WAAPI maps direction before keyframe easing and GSAP flips the raw ratio before easing (`gsap-core.js`), so curves are never mirrored at load.

### Types

```rust
pub struct Timeline { pub version: u32, pub timing: Timing,
                      pub markers: Vec<Marker>, pub nodes: Vec<Node> }
pub struct Timing { pub duration_ms: f64, pub rate: f64, pub loop: LoopMode }
pub enum LoopMode { Once, Cycle, PingPong }
pub struct Marker { pub name: String, pub t_ms: f64 }
pub enum Node { Group(Group), Track(Track) }

pub struct Group { pub id: String,
                   pub start_ms: f64,             // placement in the parent
                   pub timing: Timing,            // duration and rate here
                   pub remap: Option<Box<Track>>, // local ms -> child ms
                   pub nodes: Vec<Node> }

pub struct Track { pub id: String,
                   pub kind: ValueKind,       // Scalar, Vec2, Vec3, Color, Path, Raster, Cells
                   pub default_blend: Blend,
                   pub keys: Vec<Key> }       // sorted, at least one

pub struct Key { pub t_ms: f64, pub value: Value,
                 pub ease: Ease,                // outgoing segment only
                 pub hold: bool,
                 pub blend: Blend,              // ignored when hold
                 pub spatial: Option<Spatial> } // outgoing segment, vector values

pub enum Blend { Interpolate, Crossfade, Hold }
pub struct Spatial { pub c1: Vec<f64>, pub c2: Vec<f64> } // absolute points

pub enum Ease { Linear,
                Cubic { x1: f64, y1: f64, x2: f64, y2: f64 },
                Steps { count: u32, at: StepAt },
                Points(Vec<[f64; 2]>),         // css linear() and baked curves
                Spring { stiffness: f64, damping: f64, mass: f64, settle_ms: f64 } }

pub enum StepAt { Start, End }     // jump-start, jump-end
pub enum Value { Scalar(f64), Vector(Vec<f64>), Color([f64; 4]),
                 Path(PathData), Raster(RasterRef), Cells(CellGrid) }
```

`spatial.c1` and `c2` are absolute, so no key refers to another key; Lottie's relative form converts as `c1 = s_k + to`, `c2 = s_{k+1} + ti`.

`Spring` must reach exactly 1 at `u = 1` after `settle_ms`, and `Points` must run from `(0, 0)` to `(1, 1)`; a spring without a settle time is rejected.

Iteration count stays out of `Timing` because `sample` is total in `t`. The frame plan `FramePlan { frame_ms, passes }` yields `round(passes * duration_ms / frame_ms)` times, where `cycle` drops the duplicate end frame and `pingpong` appends the reverse leg without endpoints.

### Time mapping

One function serves the root and every group.

```
u = (t - start_ms) * rate
u = match loop { Once => clamp(u, 0.0, duration_ms),
                 Cycle => u.rem_euclid(duration_ms),
                 PingPong => { let p = u.rem_euclid(2.0 * duration_ms);
                               if p <= duration_ms { p } else { 2.0 * duration_ms - p } } }
u = remap.map(|r| r.sample_scalar(u)).unwrap_or(u)
```

The root uses `start_ms = 0` and no remap. A group with `duration_ms == 0` skips the mapping and evaluates its children at 0, a still image. `Once` clamps at both ends, matching Lottie's first-value-before-start and last-value-to-the-end rule, and SMIL `fill="freeze"`.

### Key lookup

For a track, let `i` be the last index with `keys[i].t_ms <= t`. When `i` is the last index or `keys[i].hold` is set, the value is `keys[i].value`. Otherwise let `a = keys[i]`, `b = keys[i + 1]`, `u = (t - a.t_ms) / (b.t_ms - a.t_ms)`, `p = a.ease(u)`, and combine by `a.blend`.

`p` is not clamped: handles with `y` outside `[0, 1]` overshoot on purpose, as Lottie and CSS allow. A vector with `spatial` evaluates the cubic `a.value, a.spatial.c1, a.spatial.c2, b.value` at arc length `p * L`, `L` from a polyline; the shape matches players, not their exact numbers.

### Canonical JSON input

```json
{
  "version": 1,
  "timing": {"duration_ms": 2400, "rate": 1.0, "loop": "cycle"},
  "nodes": [
    {"track": {"id": "logo", "kind": "vec2", "default_blend": "interpolate", "keys": [
      {"t_ms": 0, "value": [0, 0], "ease": {"cubic": [0.33, 0, 0.67, 1]},
       "spatial": {"c1": [42.7, 0], "c2": [120, 160]}},
      {"t_ms": 800, "value": [160, 160], "ease": "linear"},
      {"t_ms": 1200, "value": [160, 160], "hold": true},
      {"t_ms": 1200, "value": [0, 160], "ease": "linear"}]}},
    {"group": {"id": "badge", "start_ms": 400,
      "timing": {"duration_ms": 600, "rate": 1.0, "loop": "pingpong"},
      "nodes": [{"track": {"id": "badge.scale", "kind": "vec2", "keys": [
        {"t_ms": 0, "value": [1, 1], "ease": {"cubic": [0.2, 0, 0.4, 1]}},
        {"t_ms": 600, "value": [1.3, 1.3]}]}}]}}
  ]
}
```

The two keys at 1200 are the one legal duplicate: the first carries `hold`, so the value jumps there.

## Invariants and the tests that pin each one

| # | Rule | Test |
|---|---|---|
| I1 | Stable sort by `t`; a shared `t` only with `hold` on the earlier key; three keys rejected. | `out_of_order_keys_are_sorted`, `duplicate_without_hold_rejected` |
| I2 | Lookup takes the last key with `t_ms <= t`. | `lookup_uses_upper_bound`, `duplicate_time_selects_later_key` |
| I3 | Values clamp outside the key range, negative `t` included. | `before_first_key_is_first_value`, `after_last_key_is_last` |
| I4 | The last key's `ease` has no effect; changing `keys[i].ease` changes only `[keys[i].t, keys[i+1].t)`. | `last_key_ease_is_ignored`, `easing_change_is_local` |
| I5 | `hold` ignores `ease` and `blend`; the jump lands at the next key's time. | `hold_ignores_curve`, `hold_jump_at_next_key_time` |
| I6 | A one-key track is constant; no eased segment has zero length. | `single_key_track_is_constant`, `zero_length_without_hold_rejected` |
| I7 | `cycle`: `sample(t + n * d) == sample(t)`; the frame at `d` is never emitted. | `cycle_period`, `cycle_endpoint_excluded` |
| I8 | `pingpong`: `sample(2d - t) == sample(t)`, period `2d`. | `pingpong_mirror`, `pingpong_period` |
| I9 | `once` clamps and never wraps. | `once_does_not_wrap` |
| I10 | `rate = -1` mirrors time; curves are traversed backwards, not rewritten. | `rate_minus_one_mirrors`, `reverse_ease_out_reads_as_ease_in` |
| I11 | Nested groups compose as `(t - start) * rate`, wrap, remap; two equal nested cycles equal one. | `nested_equals_flattened`, `remap_reversed_plays_backwards` |
| I12 | `sample` never quantises: half-frame times interpolate, negative times are legal. | `half_frame_time_interpolates`, `negative_time_clamps` |
| I13 | Cubic handles need `x1, x2` in `[0, 1]`, allow any `y`, satisfy `e(0) = 0`, `e(1) = 1`. | `cubic_x_range_rejected`, `overshoot_value_allowed` |
| I14 | A spring ends at exactly 1 with near-zero velocity at `settle_ms`. | `spring_settles` |
| I15 | A spatial segment advances by arc length. | `spatial_quarter_time_is_quarter_length`, `collinear_tangents_match_linear` |
| I16 | Seam validation reports position jumps, slope mismatches and the ping-pong velocity flip. | `seam_report_flags_jump`, `seam_report_flags_slope_mismatch` |

## Conversion notes

**Lottie.** Time is `(t - ip) / fr * 1000`; duration is `(op - ip) / fr`, and `op` is exclusive. `s` becomes `value`; exports that omit `s` on the last key read `e` from the previous key, the `nextKeyData.s || keyData.e` fallback. `o` and `i` become `ease.cubic`; per-component handle arrays collapse to component 0 or split the track. `h: 1` becomes `hold`. `to` and `ti` become absolute `spatial.c1` and `c2`.

`a: 0` becomes a one-key track. Layer `st` and `sr` become a group's `start_ms` and `rate = 1 / sr`; precomposition `tm` becomes `remap`.

Lost: expressions, per-component easing, split position dimensions, text animators, effect stacks, the exact arc-length approximation. Duplicate times follow I1, which is narrower than the format: lottie-spec permits two keys at one `t` and leaves the value to the implementation, and lottie-web renders the later one.

**CSS animations and WAAPI.** Offset times duration gives `t_ms`; offsets are computed first, with missing ends at 0 and 1 and null offsets spread evenly. A missing 0 or 100 percent keyframe takes the element's computed value, which the model lacks: materialise it or reject.

A keyframe timing function becomes `ease`, and the one on `to` is dropped, as the spec requires. Duplicate selectors merge first. `direction: reverse` becomes `rate = -1`; `alternate` becomes `pingpong`, and `alternate-reverse` becomes `pingpong` with `rate = -1`.

Lost: `animation-composition` add and accumulate, effect `composite`, `iterationComposite`, `fill-mode: none` and `backwards`, and non-linear effect-level `easing`.

**SMIL.** `dur` seconds become ms; `values` with `keyTimes` become keys at `keyTime * dur`, and without `keyTimes` the values are evenly spaced. `keySplines[i]` moves to `keys[i].ease`, since the model is key-indexed and SMIL is interval-indexed; its four values are bounded to `[0, 1]`, so no SMIL import overshoots.

`calcMode="discrete"` sets `hold` on every key, losing mixed holds and ramps. `calcMode="paced"` ignores `keyTimes` and `keySplines`, so key times must be regenerated from path length.

Lost: `additive="sum"`, `accumulate="sum"`, event and syncbase `begin`, `fill="remove"`, and nesting, since SVG SMIL has no time containers and `begin` chains flatten. Repeats become frame-plan passes; ping-pong must be baked into `values` and `keyTimes` first.

**GSAP.** Each tween is one segment: start from the position parameter resolved in the parent, duration as segment length, `ease` as `ease`. Labels become markers and group `start_ms`; `<` and `>` resolve against the previous tween at import. `timeScale` becomes `rate`; nesting multiplies, and the documentation's example is 2 s at 0.5 inside a 0.5 timeline taking 8 s. `repeat` with `yoyo` becomes a ping-pong frame plan.

`yoyoEase`, which substitutes a curve for the reverse leg, has no field: one curve per segment. `smoothChildTiming`, on by default, moves children when a parent's `timeScale` changes, so only the resolved timing imports. Lost: `CustomEase` unless sampled into `ease.points`, callbacks, and per-child control after import.

**Rive.** Times are seconds, and keys snap to a 60 fps grid. The first key of a pair carries the interpolation: `Linear` becomes `ease.linear`, `Hold` becomes `hold`, `Cubic` becomes `ease.cubic`. `Elastic` is parameterised by amplitude and period, which does not map onto stiffness and damping; bake it into `ease.points` or fit a spring.

One-Shot, Loop and Ping-Pong become `Once`, `Cycle` and `PingPong`; the work area becomes a group's `start_ms` and `duration_ms`; negative speed becomes a negative `rate`. Lost: state machines, transitions, blend weights, inputs, and anything that depends on runtime state.

**Everything.** Two capabilities have no home here and need a decision: additive and accumulative composition, present in WAAPI and SMIL, absent from Lottie, GSAP and Rive; and per-keyframe composite operations, which need an underlying value per track. A sampler returning one value per track cannot express either without an animation stack above it. The alternative is `Track.compose: Compose { Replace, Add, Accumulate }`, evaluating tracks in order against the current frame.

## References

- airbnb/lottie-web, `player/js/utils/PropertyFactory.js` (`interpolateValue`, `getValueAtCurrentTime`), `player/js/utils/bez.js` (`buildBezierData`): https://github.com/airbnb/lottie-web
- lottie-spec, `docs/specs/properties.md`, `composition.md`, `layers.md`, `schema/properties/*-keyframe.json`: https://github.com/lottie/lottie-spec
- ThorVG, `src/modules/lottie/tvgLottieProperty.h`, `tvgLottieInterpolator.cpp`: https://github.com/thorvg/thorvg
- CSS Animations 1 3.1, Animations 2 4.10, Easing 1 2.2 and 2.3: https://drafts.csswg.org/css-animations-1/ , https://drafts.csswg.org/css-animations-2/ , https://drafts.csswg.org/css-easing-1/
- Web Animations 1, sections 4.7.6, 4.7.7, 5.3.1, 5.3.3, 5.3.4, 5.4.4: https://drafts.csswg.org/web-animations-1/
- SMIL Animation, sections 2.2, 3.2.3, 3.3: https://www.w3.org/TR/smil-animation/
- GSAP position parameter, `timeScale()`, `yoyo()`: https://gsap.com/resources/position-parameter/ , https://gsap.com/docs/v3/GSAP/Timeline/timeScale()/ , https://gsap.com/docs/v3/GSAP/Tween/yoyo()/
- GSAP 3.13 `gsap-core.js`: https://unpkg.com/gsap@3.13.0/gsap-core.js
- Rive Keys, Interpolation, Timeline: https://rive.app/docs/editor/animate-mode/keys , https://rive.app/docs/editor/animate-mode/interpolation-easing , https://rive.app/docs/editor/animate-mode/timeline

Unverified: how Rive parameterises nested artboard time, and whether its runtime mirrors the eased progress on a ping-pong pass (the editor documentation describes playback direction only). The Lottie tangent convention is inferred from two independent players and from lottie-web's collinearity test on `e + ti`; the schema does not state the anchor.
