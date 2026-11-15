# 06-tween-properties

Status: specified, not implemented. Evidence and costs: docs/research/tween-property-types.md.

## 6.1 The type set

| type | value | interpolation |
|---|---|---|
| number | `f64` | any easing |
| opacity | `f32` in 0 to 1 | any easing, clamped |
| transform | translate, rotate, scale, skew | matrix compose, decomposed for easing |
| colour | RGBA8, premultiplied | component interpolation in linear light |
| path | list of subpaths of cubic segments | correspondence-based morph |
| raster | reference to a prepared image plan | crossfade or affine, renderer side |
| ascii grid | cells of density plus colour | density field, glyph chosen after |

## 6.2 Prepare against sample

`prepare` is allowed to be slow, to allocate and to fail. `sample` must be fast, allocation free and total.

Prepare does all the work that depends on the whole track: parsing, normalising, sorting, resampling a path to matching structure, building the arc-length table, choosing the rotation offset, and computing spring settle times. Sample does a lookup, an interpolation and a compose.

This split is what makes the two-pass frame flow in part 03 possible: the palette pass and the render pass share one prepared plan and pay the expensive work once.

## 6.3 Paths

- Commands are normalised to absolute cubics, so line, quadratic and arc segments become one form.
- Two paths morph only when their subpath count and their per-subpath command count match. That is the condition the whole design rests on.
- Resampling follows flubber: sample each subpath by arc length at the same number of points, then choose the start point offset and the direction that minimises total distance. The offset search is quadratic in point count and runs at prepare; it costs 1 to 3 ms per pair at a few hundred points.
- Per frame the work is linear in point count and costs microseconds. The cubic is evaluated, not resampled again.
- A mismatch is not repaired by guessing. Prepare fails with a message naming the path and the counts, and the caller can ask for a dissolve instead.
- Holes are matched by containment and then by the same resampling rule; a hole that exists in one key only is grown from a zero-area path at the same centroid.

## 6.4 Raster

Raster is the only per-frame hotspot. Measured at 1080p: crossfade 4.3 ms, bilinear resample 21 ms, bicubic resample 64 ms. Numbers, transforms and paths are microseconds by comparison.

The crate therefore plans raster work and the renderer performs it. A prepared raster plan states source, fit, crop, zoom and blend for each key, and the renderer executes it against a decoded image cache. The crate never decodes and never resamples.

A mesh warp is out of the first version (09, row 6). It needs hand-placed correspondences that the keyframe format does not carry, and it costs about 20 ms per 1080p frame.

## 6.5 ASCII grids

- A grid is a fixed number of columns and rows; motion below one cell is impossible.
- Interpolation happens on a density field, not on glyph identities: each cell carries a density in 0 to 1, the value is interpolated, and the glyph is chosen from the ramp after interpolation.
- Colour per cell interpolates as colour.
- A glyph atlas can be prepared when the same grid is drawn many times; it changes rendering cost, not the model.
- Glyph identity is not interpolable, so a grid morph between different text is a density morph, not a letter morph.

## 6.6 Emission

`sample` returns typed geometry, never strings. Formatting a path to an SVG `d` attribute costs 30 to 50 times the arithmetic of the interpolation itself (0.09 ms at 500 points against about 3 microseconds), so formatting happens once per exported document, not per frame.

The same rule applies to colour strings, transform strings and numbers: the renderer formats, the crate computes.

## 6.7 Tests

- A path pair with matching structure morphs without a jump at either endpoint.
- A pair with mismatched counts fails at prepare with the counts in the message.
- A crossfade of two rasters produces the same pixels as a direct blend of the two decoded frames.
- A density field interpolation reaches the ramp endpoints exactly.
- Sample performs no allocation: a counting allocator sees zero allocations in the sample loop.
