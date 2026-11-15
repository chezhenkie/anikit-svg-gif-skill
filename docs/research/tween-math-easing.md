# Tween maths, easing solvers and the cost of one evaluation

## Scope

The future `tween` crate takes a validated timeline and returns one value per animated property at a requested time. This note covers the arithmetic: bezier solvers as shipped, spring closed form versus integration, steps and `linear()`, interpolation per value domain, splines beyond two keys, and time precision. Method names are in `docs/keyframe-tween-animation-info.md`; costs are measured in Node unless marked est.

## Findings

| Method | Algorithm | Cost per call | Precision notes | Source |
|---|---|---|---|---|
| Bezier, WebKit and Chromium | 11-sample x table, linear guess, 4 Newton steps, bisection fallback | 2-3 evals typical, 26 worst (measured) | 1e-7 Newton, 1e-10 bisection; extrapolates past [0,1] | `UnitBezier.h`, `cubic_bezier.cc` |
| Bezier, gre/bezier-easing | closed form for u=1/t, Cardano or trigonometric | 1 cbrt or acos, 15-25 ns est. | `D > 1e-12*h*h` branch, result clamped | `src/index.js`, `x2t` |
| Bezier, motion | Newton in a bisection bracket, forward-difference slope | 1-5 iterations, 15-30 ns est. | `x*x < 1e-28`, cap 40, step 1e-7 | `easing/cubic-bezier.ts` |
| Steps | `floor(steps*t + offset)`, clamp, divide by jumps | 3-5 ns est. | offset 1 for start, jump-start, jump-both | `StepsTimingFunction::GetValue` |
| linear() points | binary search plus interpolation | O(log n), 6 flops | inputs in percent, equal inputs take the later | `LinearTimingFunction::GetValue` |
| Spring, closed form | analytic underdamped and critical; motion adds overdamped | 1 exp + 2 trig, 40-80 ns est. | WebKit treats z>=1 as critical | `SpringSolver.h`, `generators/spring.ts` |
| Spring, integrated | semi-implicit Euler, 1 ms substep | 8 flops per ms | stateful, frame-rate dependent | `SpringValue.ts`, `advance` |
| Spring to linear() | sample generator, 4 decimals | 30 ms, about 33 points/s | resolution limits accuracy | `waapi/utils/linear.ts` |
| Rotation 3D, 2D | slerp, sign flip when dot < 0; angles numeric | 1 acos + 2 sin; 1 flop | 350deg to 10deg goes -340deg | `quaternion.cc`, `css-transforms-1` |
| 2D affine | Gram-Schmidt, then `atan2`; recompose in order | 2 sqrt + atan2 + sincos, 40-60 ns est. | negative determinant flips the smaller scale | `matrix44.cc`, `Transform::Compose` |
| Colour | convert, premultiply, blend, unpremultiply | sRGB 8 mul/add; OKLab 3 cbrt, 40-70 ns est. | mismatch goes to OKLab, hue shorter | `color.cc`, `InterpolateColors` |
| Splines | Catmull-Rom tangents or Steffen limiter | ~15 flops per component | centripetal `|dp|^0.5`, chordal `|dp|` | `CatmullRomCurve3.js`, `monotone.js` |
| Arc length | adaptive Gauss-Legendre 8/16/24 | 16-48 sqrt, 100-300 ns est. | error 2.5e-6, 1.5e-11, 3.5e-16 | `kurbo/src/cubicbez.rs` |

## The solvers

### Cubic bezier

All four build the same coefficients (`cx = 3*p1x`, `bx = 3*(p2x-p1x)-cx`, `ax = 1-cx-bx`, same for y) and evaluate with Horner.

WebKit and Chromium precompute `splineSamples[i] = sampleCurveX(i/10)` for 11 entries, interpolate t inside the bracketing segment, and run at most 4 Newton steps; only if those fail do they bisect the bracket the table gave them. Newton stops at `min(1e-7, epsilon)` and bisection at `max(1e-10, epsilon)`, so the caller's epsilon only loosens the fallback. WebKit's `solve` extrapolates outside [0,1] with the constructor's end gradients; Chromium asserts x in [0,1] in debug builds only. WebKit passes `epsilon = 1.0 / (1000.0 * duration)` (`TimingFunction::transformProgress`), the precision needed to hold the output to a millisecond; Chromium passes a fixed 1e-7.

Measured over 1001 x values: ease needs at most 3 Newton steps, 2.86 evaluations on average; ease-in-out 2 and 2.00; ease-in (0.42,0,1,1) and ease-out (0,0,0.58,1) hit the 4-step cap and fall back to 26 bisections, 2.59 on average.

gre's rewrite solves for `u = 1/t` as a cubic with Cardano or the trigonometric form: one `cbrt` or `acos`, no loop, harder to verify. motion drops the table, brackets Newton with bisection and estimates the slope by forward difference at 1e-7, doubling evaluations per iteration; its 1e-14 tolerance and cap of 40 make it the most accurate and most expensive.

### Steps

Chromium reduces all four positions to a start offset (1 for start, jump-start, jump-both; 0 otherwise) and a jump count (steps, +1 for jump-both, -1 for jump-none), then floors, applies the left-limit decrement at exact boundaries, clamps and divides. That gives `steps(4, jump-none)` three intervals, `steps(4, jump-both)` five, and `step-start` equal to `steps(1, start)`. WebKit implements the spec pseudocode directly, before-flag decrement included, so both agree at boundaries. `jump-none` with n=1 is invalid and belongs in load-time validation.

### Springs

Closed form for a unit step with zero initial velocity:

```
underdamped z<1: 1 - exp(-z*w*t) * (cos(wd*t) + (z*w/wd)*sin(wd*t)),  wd = w*sqrt(1-z*z)
critical    z=1: 1 - exp(-w*t) * (1 + w*t)
overdamped  z>1: two exponentials at rates w*(z -/+ sqrt(z*z-1))
```

WebKit stores `m_w0 = sqrt(stiffness/mass)` and `m_zeta = damping/(2*sqrt(stiffness*mass))`, treats every z>=1 case as critical, and samples at `progress * duration` seconds, so a segment shorter than the settle time ends away from the target. motion handles all three regimes with initial velocity; `findSpring` caps Newton at 12 iterations, the bounce map is `1-bounce` clamped at 0.05, and the defaults give z = 0.5 and 16% overshoot.

react-spring integrates, splitting each frame delta into 1 ms steps: `a = (-tension*1e-6*(x-target) - friction*1e-3*v)/mass`, then `v += a*1ms; x += v*1ms`. Measured against the closed form at w=10, z=0.5, maximum error over 2 s is 7.0e-2 of travel at 1/60 s steps and 4.1e-3 at 1 ms, first order in the step. At w=100 the 1/60 s integrator diverges to 4e25 inside a second and 1 ms still carries 4.1e-2 error; RK4 gives 5.3e-6 at w=10 but 7.9e-2 at w=100. A seek-based evaluator must use the closed form: cheaper, frame-rate independent and reproducible, while an integrator makes the value depend on the sequence of `advance` calls.

Settle time is load-time work: the underdamped envelope is `exp(-z*w*t)/sqrt(1-z*z)`, so a band e gives `t = -ln(e*sqrt(1-z*z))/(z*w)`; 2% is `4/(z*w)`. Baking to `linear()` is a sampling decision, not physics: motion emits `round(y*10000)/10000` at `max(round(duration/30ms), 2)` points, about 33 per second. A crate that hands frames to an encoder should call the closed form at the frame time instead.

## Interpolating each domain

Scalars and vectors use `a + (b-a)*t` componentwise; the `a*(1-t)+b*t` form loses precision when both ends are large.

CSS interpolates two `rotate()` functions numerically because they share a name and argument count (`css-transforms-1`), so 350deg to 10deg travels -340deg. 3D motion goes through matrices and takes the short arc because `Quaternion::Slerp` negates one quaternion when the dot product is negative. Shortest arc is the sane default for a keyframe tool; the wrap is `d - 360*round(d/360)`.

2D affine decomposition is needed only when two transform lists cannot be matched function by function. It is a QR factorisation by Gram-Schmidt: x scale from column 1, project it out of column 2, y scale from the rest, shear is what remains, rotation is `atan2(m12, m11)`. A negative determinant negates x scale when `m11 < m22`, else y scale. Skew versus rotation plus non-uniform scale is underdetermined, so a decompose/recompose pair can introduce a skew the endpoints never had; matching function lists componentwise avoids it. Singular matrices have no decomposition: Chromium returns `nullopt` and does not render, so return the unblended matrix or an error, never NaN.

Colours: `Color::InterpolateColors` converts both ends to the interpolation space, premultiplies by alpha, blends and unpremultiplies. Legacy sRGB endpoints stay in gamma-encoded sRGB; only mismatched spaces are promoted to OKLab (`SetupColorInterpolationSpaces`). Premultiplied blending stops a fade to transparent dragging colour through grey, and the compositor does the same in 8-bit in `gfx::Tween::ColorValueBetween`. OKLab costs three cube roots and two 3x3 products per end, sRGB four multiply-adds; for 8-bit GIF output the OKLab cost is invisible next to quantisation.

## Splines beyond two keys

A keyed list with per-segment easing is not a spline: position is continuous but velocity jumps at every key unless the easings match. Catmull-Rom derives each tangent from its neighbours (uniform `0.5*(p[i+1]-p[i-1])`, centripetal with knot deltas `|dp|^0.5`, chordal with `|dp|`) and hits every key; centripetal avoids the cusps uniform spacing produces. Monotone cubic limits the tangent with Steffen's rule, so the curve never leaves the range of its keys: right for opacity and scale, wrong for arcs and bounces. Both are cubic Hermite once tangents exist, about 15 flops per component, with tangents computed once at load.

Arc length is only needed for constant speed along a path, not for scalars between keys. kurbo's `arclen` runs an 8-point estimate, checks the error, then escalates to 16 or 24 points or subdivides; at 8 to 24 square roots it is too slow per frame. Sample `s(t)` at 32-64 points once per segment and binary-search plus interpolate per frame.

## Precision and determinism

f32 has a 24-bit significand, so representable millisecond values coarsen with the timeline: spacing is 0.0039 ms at 60 s, 0.25 ms at 1 h and 4 ms at 12 h. Measured, adding 16.6667 ms 216,000 times in f32 (one hour at 60 fps) drifts 5392 ms, 0.15%; f64 stays within 1e-9 ms. A seek-based evaluator avoids the loop by computing `t = frame_index * frame_ms` once. Chromium agrees: times are `base::TimeDelta` (int64 microseconds) and progress is one double division of exact integers (`GetTimeValues`). Store times as i64 microseconds and convert once to f64.

Cross-platform determinism is achievable but not automatic. `+ - * /` and `sqrt` on f64 are correctly rounded and identical on x86-64 and aarch64. `exp`, `sin`, `cos`, `atan2` and `cbrt` come from the platform libm and differ in the last bits between the MSVC UCRT, glibc and macOS, so springs, OKLab and quaternions are not automatically bit-identical; pinning the pure-Rust `libm` crate fixes that for a few percent on the spring path. Avoid `mul_add` in shared code. (Unverified: rustc's contraction policy; assume only explicit `mul_add` contracts.)

Release builds add two traps: integer overflow wraps instead of panicking, and `debug_assert!` disappears. A timeline checked only by assertions can carry non-monotonic times, zero-length segments or NaN easings into a release binary, where the output is a blank frame rather than a panic.

## Recommended design

One compiled timeline, one pure function, no state between frames.

```rust
pub struct Segment { t0_us: i64, t1_us: i64, ease: Ease, values: Range<u32> }
pub enum Ease {
    Linear,
    Bezier { ax: f64, bx: f64, cx: f64, ay: f64, by: f64, cy: f64,
             samples: [f64; 11], start_grad: f64, end_grad: f64 },
    Steps { steps: f64, offset: f64, jumps: f64 },
    Spring(Spring),   // closed-form coefficients for one of three regimes
}
pub fn sample(tl: &Timeline, t_us: i64, out: &mut [f32]) -> Result<(), Error> {
    for (slot, track) in out.iter_mut().zip(&tl.tracks) {
        let seg = track.segment_at(t_us);              // partition_point on i64
        let x = (t_us - seg.t0_us) as f64 / (seg.t1_us - seg.t0_us) as f64;
        *slot = track.lerp_into(seg, seg.ease.progress(x));   // shortest arc, premultiplied
    }
    Ok(())
}
```

- Compile easing once: coefficients and samples are constants of the segment, never re-derived per frame.
- For hundreds of frames of one segment, warm-start Newton from the previous t; x increases on a monotone curve, so one step usually suffices. (Est., not measured.)
- Resolve the segment with `partition_point` and special-case t on a boundary, so it returns the key value, not the limit of the previous segment.
- Flat arrays and an enum, not `Vec<Box<dyn Ease>>`: dispatch per key defeats inlining, 200 ns instead of 20 ns per call.

## Cost model

At 1080p the tween layer is not the bottleneck. A 1080p RGBA frame is 8.3 MB, so 300 frames push 2.5 GB through the encoder. A realistic scene (50 tracks x 8 properties) evaluates 400 values at 20 ns, 8 us per frame, against 5-30 ms of raster.

| Operation | Cost |
|---|---|
| Bezier, table hit, 2 Newton steps | 2.0 evals measured, 10-20 ns est. |
| Bezier, flat-tangent fallback | 26 bisections measured, 150-250 ns est. |
| Steps | 3-5 ns est. |
| Spring, underdamped | 1 exp + 2 trig, 40-80 ns est. |
| Spring, integrated at 1 ms | 8 flops per ms of delta |
| Matrix decompose + recompose | 40-60 ns est. |
| Colour, sRGB / OKLab | 5-10 ns / 40-70 ns est. |
| Arc length, table / quadrature | 6-10 ns / 100-300 ns est. |

The 11-sample table costs 11 cubic evaluations to build, about 4-5 solves, so it repays itself after a handful of samples of the same curve and occupies one cache line. Allocation discipline dominates the constant factor: a per-frame `Vec` for 400 values is 400 allocations if boxed, 1 if it is a reused slice.

## Tests

1. Bezier golden values: five named curves and 20 random ones at 1001 x values against Newton at 1e-15, tolerance 1e-9; x=0 and x=1 return exactly 0 and 1.
2. Bezier edge cases: `(0,0,0,0)` and `(1,1,1,1)` are linear, `(0,0,1,1)` equals x, y outside [0,1] overshoots and stays monotone in x, x=-0.5 or 1.5 follows the end gradients.
3. Steps matrix: all four positions for n in 2..8 at t=0, 1/n, 1 and either side of each boundary, including the before flag; reject jump-none with n=1.
4. Spring against RK4 (z in {0.2,0.5,1.0,2.0}, w in {5,20}, 1e-5 s steps, 1e-6 tolerance) and against the settle band.
5. Matrix round trip within 1e-12; singular returns an error; 180deg and negative determinant keep the sign on the right axis.
6. Colour and time: a fade keeps hue, alpha 0 does not divide by zero, OKLab round trip within 1/255; a key time returns the key value exactly; a 12-hour timeline keeps every frame boundary.
7. Determinism: hash the f64 output of the bezier and steps paths across the three platforms; for springs and OKLab require 1e-12, or pin `libm` and require bits.

## References

- WebKit: `Source/WebCore/platform/graphics/UnitBezier.h`, `SpringSolver.h`; `Source/WebCore/platform/animation/TimingFunction.cpp`.
- Chromium: `ui/gfx/geometry/cubic_bezier.cc`, `quaternion.cc`, `matrix44.cc`, `transform.cc`; `ui/gfx/animation/keyframe/timing_function.cc`, `keyframed_animation_curve-inl.h`.
- Blink: `third_party/blink/renderer/platform/graphics/color.cc`; `core/animation/interpolable_style_color.cc`.
- JS and Rust: gre/bezier-easing `src/index.js`; motion `packages/motion-utils/src/easing/*`, `packages/motion-dom/src/animation/generators/spring.ts`; react-spring `SpringValue.ts`; three.js `CatmullRomCurve3.js`; d3-shape `src/curve/monotone.js`; kurbo `kurbo/src/cubicbez.rs`.
- Specs: `css-easing-1` and `css-transforms-1`.

## Crates read, not adopted

- `keyframe` 1.1.1 (MIT): f64 time plus `Arc<dyn EasingFunction>` per key; its `BezierCurve` is an f32 port of gre's old solver, so accuracy is capped and each key allocates.
- `interpolation` 0.3.0 (MIT): Penner easing enum plus `Lerp`; no bezier, steps, springs or time type.
- `easer` 0.3.0 (MIT): Penner functions with `(t, b, c, d)` arguments, `no_std`, last release 2022.
- `bevy_tweening` 0.17.0: `Tweenable::step(&mut self, delta: Duration, ...)` advances state; good loop bookkeeping, unusable as a clock.
- `glam` 0.34.3: `Quat::slerp` is fast, but `acos_approx` is a 7-degree minimax polynomial and the SIMD paths use accelerated sin, so backend results differ.
- `kurbo` 0.13.1: best Rust reference for `CubicBez::eval`, adaptive `arclen` and `solve_cubic`; a large dependency for one call.
