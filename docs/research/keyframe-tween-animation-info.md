# Keyframe tween and interpolation research

Research notes on generating animation from keyframes. Not a specification.

## 1. Methods table

Cost assumes 640x360, about 50 frames, single core. "Fit" means usable from Python 3.14 with Pillow and numpy only.

| # | Method | Approach | Quality | Cost | Deps / license | Python fit | Failure modes |
|---|---|---|---|---|---|---|---|
| 1 | Linear (LERP) tween | Per-channel `a+(b-a)*t` on numbers, colors, opacity, transforms | Exact for constant motion | ~0 | numpy | Native | Visible corners at keys; no velocity continuity |
| 2 | Cubic Bezier easing | CSS semantics, solve x->t by Newton plus bisection | Matches CSS/WAAPI exactly | ~0 | none | Native (`numpy`, 20 lines) | Overshoot if y outside [0,1]; needs solver guard |
| 3 | Spring easing | Analytic damped oscillator, or baked to `linear()` points | Good for UI overshoot | ~0 | none | Native | Must derive frame count from settle time; per-key retiming breaks the CSS equivalent |
| 4 | Stepped/hold keys | Nearest-previous key, `steps(n)` | Intentional | ~0 | none | Native | Looks broken if applied to transforms by accident |
| 5 | SVG numeric attrs | Tween `transform` matrix, `opacity`, `fill`/`stroke` | Pixel-exact | ~0 | Playwright (Apache-2.0) for raster | Native | Matrix decompose ambiguity (skew vs rotate plus scale) |
| 6 | Path morph, compatible commands | Normalize to absolute cubic, equal command counts, interpolate control points | Good when author controls keys | ~0 | none | Native | Command-count/type mismatch causes a jump; subpath count mismatch; winding flips |
| 7 | [flubber](https://github.com/veltman/flubber) | Arc-length resample plus point correspondence plus ring rotation | Best generic 2-point morph | ~1-5 ms/pair | JS, MIT, unmaintained since 2022 | Via Node or page context | Self-intersection, holes lost, thin strokes and glyphs smear, `holes`/`separate` needed for compounds |
| 8 | [svg-path-morph](https://github.com/Minibrams/svg-path-morph) | Average path embedding, n-way weights | Good for N variants of one shape | ~1 ms | JS, MIT | Via Node | Requires identical command structure by design |
| 9 | [GSAP MorphSVG](https://gsap.com/docs/v3/Plugins/MorphSVGPlugin/) | Subdivide plus find best rotation, handles topology | Highest quality generic | ~ms/pair | JS, custom "no charge" license (not OSI) | Weak (JS only) | License review for redistribution; large bundle |
| 10 | SMIL / CSS / WAAPI in Chromium | Author `values`, `d:path()`, or WAAPI keys, then `setCurrentTime(t)` plus screenshot | Exactly browser semantics, free easing set | 5-20 ms/frame | Playwright Chromium (Apache-2.0) | Strong | Chromium-only fidelity; command mismatch jumps; `linear()`/`spring()` support varies |
| 11 | Multi-SVG keyframe diff | Match elements by `id`, diff attributes, fade unmatched in/out | Practical for real files | low | numpy plus XML | Native | No `id`s means no correspondence; element count changes force dissolve, not morph |
| 12 | Raster crossfade | `Image.blend` / alpha lerp | Clean for stills | ~1 ms | Pillow (MIT-CMU) | Native | Reads as dissolve, not motion; mid-frames look flat and semi-transparent |
| 13 | Affine/Ken Burns | Pillow `Image.transform(AFFINE, BICUBIC)` per key | Good | 1-3 ms/frame | Pillow | Native | Subpixel shimmer on thin lines and text; edge clamps stretch |
| 14 | Mesh/field morph | Beier-Neely or Delaunay plus affine, `scipy.ndimage.map_coordinates` | Good with hand-placed correspondences | 10-50 ms/frame | scipy (BSD-3) | Native | Needs correspondences; fold-over in concave regions |
| 15 | Optical flow warp | OpenCV DIS/Farneback, backward warp plus occlusion mask plus blend | Medium | 50-300 ms/frame | opencv-python (Apache-2.0) | Native | Flat vector/text has no texture, so aperture failure, tearing at edges |
| 16 | ffmpeg `minterpolate` mci | Block motion estimation plus OBMC | Medium | n/a here | ffmpeg (LGPL/GPL) | Unavailable | No ffmpeg in target; halos, warping on large motion |
| 17 | [RIFE](https://github.com/hzwer/ECCV2022-RIFE) | IFNet intermediate flow, arbitrary timestep | High on footage | 30+ fps at 720p on 2080Ti; CPU minutes/frame | torch, MIT | Yes (torch has cp314 wheels) | About 1 GB downloads, GPU need, warps text and line art |
| 18 | [FILM](https://github.com/google-research/frame-interpolation) | Multi-scale feature interpolation | High on large motion | seconds/pair | TF2, Apache-2.0, archived | Poor | Archived, TF2 on 3.14, heavy |
| 19 | [FLAVR](https://github.com/tarun005/FLAVR) | 3D conv, needs 4+ frames | High on video | 3.1 fps at 512x512 (GPU) | torch, Apache-2.0 | Poor | Cannot do 2-key eases; old torch pins |
| 20 | [butterflow](https://github.com/dthpham/butterflow) | Flow warp plus blend CLI | Medium | slow | OpenCV, MIT, archived | External binary | Archived; MP4-centric |
| 21 | ASCII glyph re-quantize | Interpolate density field, remap to ordered glyph ramp per frame | Smooth, reads as ASCII | ~1-3 ms/frame | Pillow | Native | Shimmer if ramp is not ink-ordered; glyph flicker on ties |
| 22 | ASCII per-cell glyph crossfade | Pre-render glyph atlas, lerp cell bitmaps, keep grid | Smoothest | ~3-6 ms/frame | Pillow | Native | Cell text becomes gray mush mid-tween; needs monospace metrics fixed |
| 23 | ASCII text morph | Rewrite glyphs in sweep/noise order | Stylized | ~0 | none | Native | Not temporal smoothing; unreadable at speed |
| 24 | Global palette and fixed quantization | Build one palette from sampled frames, map all frames | Stable, no flicker | ~10 ms total | Pillow, or [gifsicle](https://github.com/kohler/gifsicle) (GPL-2.0) | Native / external | Dither noise inflates LZW; delta rectangles stop helping |
| 25 | Motion blur | N subframe renders averaged per output frame | Better fast motion | N times render | same | Native | Smears with only 40 ms per frame; cost linear |

## 2. Keyframe schema

The canonical input shape is specified in `docs/specsheet/04-tween-model.md`, section 4.6. The sketch that started this note is superseded and has been removed so the repository holds one copy of it.

## 3. Pipeline

1. Load and validate: normalized timeline struct; reject non-monotonic `t`, unknown kinds, missing files.
2. Plan frames: `frame_ms` from fps (40 ms default); `n = round(duration/frame_ms)`; `cycle` drops the duplicate last frame, `pingpong` appends the reverse without endpoints.
3. Sample as a pure function of `t`: resolve each track to a typed scene state. Vector tracks produce an SVG document per frame; raster tracks produce composited RGB arrays; ASCII tracks produce `(glyph, fg, bg)` grids.
4. Render: one Playwright page reused for all frames; `svg.pauseAnimations()` plus `svg.setCurrentTime(t)` for SMIL, or pause all `document.getAnimations()` and set `currentTime`. Screenshot per frame into a reused buffer. Raster and ASCII paths never touch the browser.
5. Assemble: append frames to `list[np.ndarray]`; optional N-subframe average for motion blur.
6. Palette: concatenate every k-th frame, `quantize(colors=255, method=MEDIANCUT)`, then map every frame with `dither=NONE` against that one palette (reserve index 0 for transparency).
7. Write: existing GIF writer, `duration=frame_ms`, `loop=0`, `disposal=1`, `optimize=True`. Pillow already emits delta rectangles via `_getbbox`, so a fixed palette plus no dither is what keeps size down.
8. Emit: also write the SVG/HTML file when all tracks are vector, from the same timeline (no second source of truth).

## 4. Defaults

- `fps: 25` (40 ms). 2 s is 50 frames. Never 10 ms: GIF stores 1/100 s and Chromium/Firefox clamp delays of 10 ms or less to 100 ms, so 10 ms plays 10x slow. 20 ms is the usable floor.
- Easing: `cubic-bezier(0.33,0,0.67,1)` for transforms, `linear` for loops and camera moves, `ease-out` for entrances, `hold` never implicit. Springs only with an explicit settle time.
- 2 keys for a constant move, 3 for an arc, 4 max. More keys than frames/8 is wasted.
- `loop: cycle`; use `pingpong` only when the pose at the seam differs, and then keep easing symmetric or the seam stutters.

## 5. Limits and prototype risks

- Crossfade is not morphing. Without correspondence, raster keyframes can only dissolve or warp by hand-placed control points.
- Optical flow and ML VFI are trained on natural footage; flat fills, text and 1 px strokes produce tearing and ghosting. Validate one text frame before adopting.
- Model cost is dominated by setup, not inference: torch cp314 wheels exist (2.15), but CUDA download size and cold start are incompatible with a skill that starts thousands of times.
- SVG morphing requires command and subpath parity. Multi-file keyframes therefore need `id` correspondence and an explicit fallback to opacity fades; expect the fallback to be the common case.
- GIF palette churn is the main quality risk: per-frame adaptive palettes flicker, dithering inflates LZW and defeats Pillow's delta rectangles. Prototype: same animation with global palette plus no dither vs per-frame palette, compare size and visible banding.
- ASCII sub-cell motion is impossible on a fixed monospace grid; only glyph and color change. Cell crossfade buys smoothness at the cost of legibility mid-tween.
- Loop seams: verify frame `n-1` to `0` velocity, not just position; easing at the seam must be continuous or the loop visibly hitches.
- Prototype order: numeric/transform tween, then ASCII grid, then single-path morph, then multi-SVG correspondence, then optional flow. Measure per-frame render time and output KB at each step.

## 6. Composition

Superseded by `docs/specsheet/07-tween-composition.md`. The evidence, the format comparison and the tool prior art stay in `docs/research/tween-composition.md`.
