# 06-tween-properties

Status: implemented for matched-structure path morphs, the correspondence fallback, raster plans and density grids. Hole growth and arc normalisation remain unimplemented. Evidence and costs: docs/research/tween-property-types.md.

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

Prepare does all the work that depends on the whole track: parsing, normalising, sorting, resampling a path into a common point count, building the arc-length table, choosing the rotation offset, and computing spring settle times. Sample does a lookup, an interpolation and a compose.

This split is what makes the two-pass frame flow in part 03 possible: the palette pass and the render pass share one prepared plan and pay the expensive work once.

## 6.3 Paths

- Commands are normalised to absolute cubics, so line, quadratic and arc segments become one form.
- A pair with the same subpath count, the same closed flags and the same per-subpath curve count interpolates corresponding cubics. That mode is exact and stays the first choice.
- A pair that differs only in curve counts takes the correspondence fallback (09, row 27). Each subpath pair is flattened and sampled by arc length into a common polyline: the count is `ceil(longest perimeter / 4 px)`, clamped to 32 and 1024, so the polyline stays within about a pixel of the authored outline at normal drawing sizes.
- A closed subpath tries every start offset and keeps the one with the least squared travel. An open subpath keeps its authored ends and is not rotated, because the ends of a stroke are placed on purpose.
- Direction is chosen per path, every subpath reversed or none. Reversing one ring of a compound path would fill a hole under nonzero, and ring classification is a later gap, so a path with an open subpath is never reversed.
- The offset search is quadratic in point count and runs at prepare, once per ring per direction tried: 0.02 ms at 100 points, 0.13 ms at 300, 1.5 ms at 1000. Per frame the work is linear in point count and costs microseconds.
- The fallback writes a straight cubic per pair of points, so the result is a polyline. A key time still returns the authored path itself, which makes the outline step by about a pixel as the polyline morph begins.
- Prepare reports the mode as `PathFidelity`: `Exact` for the cubic mode, `Resampled { points }` with the largest sampled count otherwise. A route that draws paths prints that line; the tween route still draws only five properties (09, row 21).
- A subpath count or closed-flag mismatch is not repaired by guessing. Prepare fails with a message naming the path and the counts, and the caller can ask for a dissolve instead.
- Hole matching by containment, and the growth of a hole that exists in one key only from a zero-area path at its centroid, remain open. Until they land, a subpath present in one key only fails.

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
- A pair that differs in curve count prepares as `Resampled`, and its mid-morph stays on the shape when the two keys start at different angles and run in opposite directions.
- A resampled morph returns both authored keys exactly at their times.
- A mismatched subpath count or closed flag fails at prepare with the path name in the message.
- A crossfade of two rasters produces the same pixels as a direct blend of the two decoded frames.
- A density field interpolation reaches the ramp endpoints exactly.
- Sample performs no allocation: a counting allocator sees zero allocations in the sample loop.
