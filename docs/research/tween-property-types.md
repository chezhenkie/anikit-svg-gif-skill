# Tween property types

## Scope

What an animation carries, how each type interpolates, and what it costs per frame. Figures are per output frame, single core, scalar Rust without SIMD, u8 RGBA, measured at 1920x1080 with a probe built here; 640x360 has 1/9 the pixels.

## Per-type table

| Type | Algorithm | Cost per frame | Exactness | Fallback | Source |
|---|---|---|---|---|---|
| number, opacity | `a + (b-a)*e(t)` | ~1 ns | exact | none | [web-animations] |
| transform 2D | decompose, lerp parts, recompose | ~50 ns | exact while decomposition is stable | matrix lerp | [css-transforms] |
| transform 3D | quaternion slerp plus lerp | ~100 ns | exact apart from the arc | matrix lerp | [css-transforms] |
| colour | premultiplied lerp in sRGB or OkLab | ~20 ns | perceptual | opaque sRGB lerp | [css-color], [oklab] |
| stroke, dash | scalar lerp; dash lists only at equal length | ~10 ns here, coverage in renderer | number exact, appearance not | discrete step | [svg-paint] |
| gradient stops | by index, else by normalised offset | ~1 us | perceptual | resample offsets, else crossfade | [svg-pservers] |
| filter params | numeric lists, discrete for seed/type/order | renderer bound | exact for matrices, perceptual for blur | hold | [svg-filters] |
| path, same structure | lerp every command parameter | O(C*P), 1-3 us | exact | none | [morph-src] |
| path, no correspondence | normalise, subdivide, resample, rotate | O(N) after O(N^2) prepare | approximate | crossfade | [flubber-rotate] |
| raster crossfade | premultiplied per-pixel lerp | 4.3 ms 1080p, 0.5 ms 360p | perceptual | hard cut | probe |
| raster affine | one resample, prefilter when shrinking | 21 ms bilinear, 64 ms bicubic 1080p | approximate | nearest | probe |
| raster mesh warp | backward map, bilinear patch per pixel | 20 ms 1080p | approximate | per-quad affine | probe |
| text | variable-axis numbers, shaped by renderer | renderer bound | exact for numeric axes | substitution steps | [css-fonts] |
| ASCII grid | lerp density, quantise to ordered ramp | 2048 cells ~0.1 ms (estimated) | perceptual after quantise | hold the glyph | probe |

Numbers, transforms and structurally matched paths are exact; colour, gradients, filters and raster perceptual; a path without correspondence approximate, with a reported fallback.

## Paths with correspondence

flubber's `interpolate(from, to, {maxSegmentLength = 10})` has four steps.

Reduce. `svgpath(d).abs()` then split on `M`. Only the first subpath survives: `pathStringToRing` takes `split(parsed)[0]`, so extra subpaths and holes are dropped. A subpath of only M/L/H/V keeps its vertices; anything with a curve, an arc or a second M goes to `approximateRing`, which samples `max(3, ceil(len / maxSegmentLength))` points at equal arc length. Curves are a polyline from here on.

Winding. `normalizeRing` drops the repeated closing point, takes the signed area and reverses when it is positive, so every ring runs the same way. Direction comes from the area sign alone; both orders are never compared. Long segments are bisected after that, except on rings that sampling already spaced.

Counts. `addPoints` raises the shorter ring to the longer, inserting at equal perimeter spacing from half a step in, so insertions miss existing vertices by up to half a step even on identical shapes. Both it and the bisection splice per point, shifting the tail each time: quadratic for a large count difference. Where two subpaths differ only in cubic count, de Casteljau subdivision at t=0.5 raises the count without changing the shape, so counts match exactly and the control points then interpolate; that beats point resampling. It fixes counts, not correspondence.

Rotation. `rotate` tries every offset, sums squared distances from `ring[(offset+i) % n]` to `vs[i]`, and splices to the best. O(N^2), once per pair: 0.02 ms at N=100, 0.13 ms at 300, 1.46 ms at 1000. It compares phase only, so a ring that fits better reversed keeps its normalised winding and takes the long way round.

Cost. Per frame, N point lerps at 4 flops each cost microseconds at N=500, against 0.09 ms measured for building the path string, 30 to 50 times the arithmetic. Per pair, sampling dominates: `getPointAtLength` searches up to 500 iterations over a 20-node Gauss-Legendre quadrature, so each sample costs hundreds of square roots. That is 1-3 ms per pair at N=200 (estimated, consistent with the 1-5 ms in the keyframe note), plus 0.1-1.5 ms of rotation, under 0.1 ms per frame over 50 frames.

Holes, direction, start. A compound path keeps its rings, matches them as groups and re-emits with a fill rule. flubber does none of this, and its winding normalisation is wrong for holes under `nonzero`: outer and hole must run opposite ways, so forcing one way fills the hole. The crate must classify rings by containment, pair each hole with its outer ring, compare hole counts per group and treat a mismatch like a subpath mismatch; a hole present at one key only can collapse to a point and fade. Reversing a ring or moving its start index leaves the filled region unchanged under either fill rule, so both are free for an isolated ring and limited only by relative winding inside a group.

One shape to several is `separate`: earcut triangulates the source ring, adjacent triangles merge until the piece count matches the targets, and pieces are matched by centroid, by permutation up to eight and by index above that.

## Paths with matched structure

svg-path-morph tokenises with `parse-svg-path`, without converting to absolute, checks that every path has the same commands in the same order and throws otherwise, then stores the parameter average and each path's difference from it. `morph(weights)` evaluates `average + sum(diff[p]*w[p])`; weights must sum to 1 or the result scales about the origin. O(C*P*N) with no geometry work, microseconds for 200 cubics, and curves survive. The price is authored alignment; relative deltas depend on command order. This is the cheap exact mode; correspondence is the expensive one, chosen in the track.

## Raster

Crossfade reads two frames and writes one: 4.3 ms at 1080p, 0.5 ms at 640x360. Bilinear resampling costs five times that, bicubic fifteen times. At GIF sizes raster tweening is affordable; at 1080p it must be tiled, streamed or left to the renderer.

Blend before resampling. Bilinear and bicubic are linear operators, so for equal-size sources under one transform `resample(lerp(a,b,t))` equals `lerp(resample(a,t), resample(b,t))` up to rounding. Measured: 4.3 plus 21 ms against 21 plus 21 plus 4.3. Two sources under one camera cost one resample; N sources cost one resample plus N-1 blends. The equality dies when a non-linear step sits between them: u8 quantisation, gamma-space blending, dithering. Keep f32 premultiplied, quantise once, and matte onto the output background, because GIF's 1-bit transparency loses everything below 50 percent alpha.

Ken Burns is one affine, so one resample; zoom is a scale and crop moves the source origin. Below scale 1, prefilter with a box or mip reduction, or detail aliases into crawling noise that a 256-colour palette makes worse. Mesh warp maps backwards, destination pixel through the grid to a source coordinate; a forward map leaves holes. The 20 ms includes a table read and four taps, so the grid is nearly free: cost is per pixel however few control points were placed. Per-quad affine is cheaper and creases at quad edges; bilinear patches remove the creases for about the affine cost.

## Colour, gradients, filters

A 50 percent blend of black and white is 128 in sRGB code value and about 188 when blended in linear light and encoded. sRGB suits flat art, linear light suits shadow, OkLab keeps saturation through the middle; the choice is recorded in the timeline so gradients and filter colours use it too.

Gradient stops with equal counts and offsets interpolate per index, exactly. Unequal counts resample by normalised offset, piecewise linear: the ramp keeps its shape, but a stop at 0.3 has no partner and loses identity. Clamp offsets to stay monotone or the renderer emits reversed ramps. `spreadMethod` and `gradientUnits` are discrete; `gradientTransform` interpolates as a matrix.

Filters split three ways: `feColorMatrix` is 20 numbers and exact; `stdDeviation` lerps smoothly but renders non-linearly, with cost following blurred area rather than radius in a box-blur approximation; `seed`, `type`, `operator` and primitive order are discrete. Stroke width lerps as a number and rasterises as coverage: between 0.5 and 2 px a half-pixel change moves every edge alpha value along the outline, which on 256 undithered levels flickers, so snap widths under about 1.5 px to 1 px or step 0.25 px with dither.

## Text and ASCII grids

The crate carries text as numbers: size, letter spacing, position, variable-axis values such as `wght` and `wdth`, which interpolate exactly. Shaping, line breaking and glyph choice stay with the renderer. Freeze line breaking at prepare, or a width change reflows words mid-tween.

An ASCII grid is a numeric type with a quantiser. Each cell holds a density or glyph index plus foreground and background colour; per frame the crate lerps those and quantises to a ramp ordered by ink coverage. 64x32 cells is 2048 lerps, microseconds of arithmetic, with the cost in blitting the atlas (estimated 0.1 ms for the grid; the keyframe note gives 1-3 ms/frame including rasterisation).

Sub-cell motion is impossible on a fixed grid. A glyph's origin sits at a cell corner, its ink is clipped to the cell, and a shift smaller than a cell overlaps the neighbour or clips, while a shift of a whole cell is a jump. Between frames only the glyph, its colours and the background change; ink cannot travel, so anything resembling sub-cell motion is a sequence of static arrangements that reads as a jump at 25 fps unless cells are small.

Both softenings have a cost. Per-cell crossfade draws both masks at `1-t` and `t`, doubling the blit and greying the cell, since two superimposed glyphs cover more than either alone; use it for a frame or two, or threshold at 0.5. Ramp morphing keeps one glyph per cell and needs a ramp monotone in coverage; near-equal entries alternate frame to frame, so break ties by index with hysteresis at the midpoint.

## Recommended type and trait design

Three cost findings drive it.

- Raster is the only per-frame hotspot (4-64 ms at 1080p against microseconds), so it needs a prepared plan that keeps pixel work outside the crate.
- Path prepare is O(N^2) in rotation and 1-3 ms per pair, but O(N) per frame, so prepare is separate, failable and cacheable.
- Formatting a path costs 30-50 times the arithmetic, so the sampler emits typed geometry.

A closed set of typed tracks over a small `Lerp` trait, in two phases; `Prop` is the wire form, `Prepared` the prepared subset:

```rust
pub trait Lerp: Clone { fn lerp(&self, b: &Self, t: f32) -> Self; }

pub struct Key<T> { pub t: f64, pub v: T, pub ease: Ease }

pub enum Ease {              // solved in O(1)
    Linear, Hold, Bezier([f32; 4]), Steps(u32),
    Sampled(Arc<[f32]>),     // springs baked at prepare
}

pub enum Track {             // sized, no boxing per key
    Number(TrackOf<f32>), Transform(TrackOf<Transform>), Color(TrackOf<Premul>),
    Stroke(TrackOf<StrokeProp>), Gradient(TrackOf<GradientProp>),
    Path(TrackOf<PathKey>), Raster(TrackOf<RasterKey>), Ascii(TrackOf<Grid>),
}

impl Track {
    pub fn prepare(&self, ctx: &Ctx) -> Result<Prepared, PrepareError>;
}

impl Prepared {
    pub fn sample_into(&self, t: f64, out: &mut Scene, scratch: &mut Scratch) -> Fidelity;
}
```

Sampling is irrelevant for most rows, so `Box<dyn Lerp>` would lose nothing measurable; a vtable call per track per frame is nanoseconds. Neither choice moves startup, which is timeline parsing and renderer launch. The enum wins on three other things: `prepare` returns typed errors instead of strings, each variant owns its scratch (rings, resample plans, pixel buffers), and keys stay inline with no allocation per key.

Monomorphisation is bounded because `TrackOf<T>` is generic over the value type alone and easing is an enum, giving one sampler copy per value type, a few KB each; making easing generic would multiply that for no gain. `f32` geometry, `f64` time, premultiplied `f32` colour quantised once, and each key carrying the easing of its outgoing segment. `Ease::Bezier` is three Newton steps with a bisection guard; springs and `linear()` curves are baked into 65 samples at prepare.

## Failure modes and what the caller is told

Prepare can fail; sample cannot. `sample_into` does not allocate, panic or return an error, so a broken property degrades visibly instead of aborting a render halfway.

`PrepareError` names the cause and the track: `TopologyMismatch { groups_a, groups_b }`, `Degenerate { ring, points }` (fewer than three distinct points, zero area or length), `Unsupported { what }` (zero arc radius, unresolvable filter reference), `AssetMissing { path }`, `NotInterpolable { a, b }` (dash lists of unequal length, differing gradient units).

`Fidelity` is returned rather than thrown: `Exact`, `Structural`, `Resampled { points, max_travel }`, `Dissolve`, `Hold`. The CLI prints one line per track that is not `Exact` before rendering. A degraded morph is a normal outcome, so the exit code stays zero; `--strict` promotes any non-`Exact` track to an error.

Approximations the caller must know:

- Path output is a polyline, and faceting shows above about 0.5 px of chord error. Endpoints are exact: t=0 and t=1 return the original geometry, at the cost of a one-frame step.
- Rotation is a heuristic over sampled points, so concave shapes can bow-tie at mid-tween; counting crossings at a few interior t values catches that, at O(N^2) per check, so only under `--strict`. Colours move through the declared space, so mixing crate-lerped and CSS-authored colours gives two midpoints for one pair. NaN in a key is rejected at load; times outside the track are clamped with a warning.

## Crate and renderer

Crate: parsing `d` and transform lists, absolute cubics, ring classification, correspondence, resampling, easing, timeline lookup, gradient stop resampling, and sampling into a `Scene` of typed values plus `Fidelity`. No pixels, fonts, anti-aliasing, filters or palette.

Renderer: stroke expansion and coverage, gradient and filter evaluation, shaping and line breaking, glyph atlas and blitting, raster kernels, colour conversion, quantisation. For raster the crate emits a plan, not an image: source reference, source and destination rect, affine, blend weights, optional mesh. The pixel loop sits behind a `raster` feature or in the binary, which keeps image code out of a vector-only run. The timeline also needs explicit fields for colour space and fill rule, since neither can be inferred and both change the output.

## References

[flubber-rotate]: https://github.com/veltman/flubber/blob/master/src/rotate.js
[morph-src]: https://github.com/Minibrams/svg-path-morph/blob/master/src/index.ts
[css-transforms]: https://www.w3.org/TR/css-transforms-1/#matrix-interpolation
[css-color]: https://www.w3.org/TR/css-color-4/#interpolation
[css-fonts]: https://www.w3.org/TR/css-fonts-4/#font-variation-settings-def
[web-animations]: https://www.w3.org/TR/web-animations-1/#animation-effect-software-model
[svg-pservers]: https://www.w3.org/TR/SVG11/pservers.html
[svg-paint]: https://www.w3.org/TR/SVG11/painting.html
[svg-filters]: https://www.w3.org/TR/SVG11/filters.html
[oklab]: https://bottosson.github.io/posts/oklab/

Matrix decomposition: Thomas, Graphics Gems II. Perceptual colour: Ottosson, OkLab; CSS Color 4 for premultiplied interpolation.

Unverified here: the estimates (1-3 ms per path pair, 1 us per gradient, 0.1 ms per ASCII grid) are counted from operations, not timed; the claim that Skia's blur cost follows area rather than radius rests on its box-blur approximation and was not measured; the ASCII estimate excludes rasterisation. The raster and rotation figures are measurements from the probe in Scope, without SIMD, and should be re-run on the target before being treated as a budget.
