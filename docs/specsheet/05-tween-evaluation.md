# 05-tween-evaluation

Status: specified, not implemented. Evidence: docs/research/tween-math-easing.md. All easing and interpolation happens inside one sample call, so its cost is the cost of a frame that carries values.

## 5.1 The easing set

| name | form | notes |
|---|---|---|
| linear | `t` | default for loops and camera moves |
| cubic-bezier | four control values, CSS semantics | solved for x, evaluated for y |
| steps | `n` steps with jump-start, jump-end, jump-none, jump-both | discrete by intent |
| hold | no interpolation | same as a duplicate key time |
| spring | mass, stiffness, damping, initial velocity | closed form only |

A spring is accepted only with an explicit settle time, because the crate must know when the value stops changing to end the segment.

## 5.2 Bezier solving

The solver follows WebKit's `UnitBezier`: Newton-Raphson from the current estimate, with a bisection fallback when the derivative is too small or the bracket collapses.

- Coefficients and the 11-sample table are compiled per segment at prepare, never per frame.
- Measured cost is two or three cubic evaluations per sample in the common case, and 26 bisections for a flat-tangent curve such as `ease-in` at a 1e-10 bracket. That worst case is why the table exists: it gives a start point close enough that Newton converges in one or two steps.
- Endpoints are returned exactly, without solving, so `t = 0` and `t = 1` cannot drift.
- Control values for y outside 0 to 1 are allowed, because overshoot is a deliberate authoring choice; the solver only requires x in 0 to 1.

## 5.3 Springs

Springs are evaluated in closed form from the damped oscillator equation, not integrated step by step.

- Semi-implicit Euler is not usable for seeking: at 60 fps it is off by about 7 percent of travel at a natural frequency of 10, and it diverges at 100. Both were measured.
- Settle time is computed at prepare from the damping ratio and the distance threshold, and it fixes the segment length and the frame count.
- Initial velocity is part of the key, so a spring can start with a flick.
- Baked `linear()` output is an export concern only; the crate never bakes to evaluate.

## 5.4 Splines over more than two keys

- Default: cubic Hermite with Catmull-Rom tangents, computed once at prepare.
- Optional: explicit per-key tangents, for formats that carry them.
- Monotone cubic (Fritsch-Carlson) is available for scalars that must not overshoot, such as opacity or a progress value.
- Arc-length parameterisation is used only where distance matters, which is path morphing (part 06), not scalar tracks.

## 5.5 Precision

- Time is `u64` microseconds; the only conversion is `t as f64 / 1e6` once per sample.
- Frame times are never `f32`: an accumulator in seconds drifts by seconds per hour at 60 fps (about 5.4 in the measured case).
- `exp`, `sin` and `cos` differ between platform maths libraries, so the crate pins `libm` where a result must be identical everywhere. Springs are the only place this matters in practice.
- Whether the compiler may fuse a multiply and add into an FMA without `mul_add` is unverified in the research. A test with fixed inputs compares the release build against a stored expected value, which settles it for each target.

## 5.6 Cost

- One scalar sample with a bezier segment: sub-microsecond, dominated by the table lookup and one Newton step.
- One spring sample: one `exp` and one `cos`, still sub-microsecond.
- Prepare per segment: tens of nanoseconds for coefficients and samples, one settle computation per spring.
- A frame that samples twenty scalar tracks costs less than the palette lookup that follows it. Evaluation is never the frame bottleneck; raster resampling is (part 06).

## 5.7 Tests

- Each easing matches a reference table generated from the CSS implementation, within 1e-6.
- Endpoints are exact for every easing, including overshooting beziers.
- A spring reaches its settle time with the value within the stated threshold and the velocity near zero.
- Monotone cubic never overshoots a key value.
- A reverse and forward pass over the same track returns the original values at matching times.
