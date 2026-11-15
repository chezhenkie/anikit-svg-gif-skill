# Multi-directional conversion research

Research notes on formats, tools and conversion paths. Not a specification.

## Scope

Fit categories: **direct** (pip-installable, already in the stack), **optional binary** (subprocess, graceful absence), **reference** (read for approach only).

## Ranked findings

| # | Project | License | Lang / stars / activity | How it works | Fit |
|---|---------|---------|--------------------------|--------------|-----|
| 1 | [Pillow](https://github.com/python-pillow/Pillow) | HPND (MIT-CMU) | Py/C, 14.0k, active | GIF/APNG/WebP read and write, `ImageSequence`, quantization, disposal control | direct |
| 2 | [Playwright](https://github.com/microsoft/playwright) | Apache-2.0 | TS/Py, active | Chromium raster of SMIL/CSS/WAAPI at seeked times | direct |
| 3 | [vtracer](https://github.com/visioncortex/vtracer) | MIT | Rust + pyo3, 7.5k, active | Color raster to stacked SVG paths; `pip install vtracer` | direct (extra) |
| 4 | [CairoSVG](https://github.com/Kozea/CairoSVG) | LGPL-3.0 | Py, 958, active | Static SVG to PNG/PDF via Cairo; no SMIL/CSS animation | direct (static) |
| 5 | [resvg](https://github.com/linebender/resvg) | Apache-2.0 OR MIT | Rust, 4.2k, active | Static SVG to PNG, byte-identical across platforms; animations explicitly unsupported | optional binary |
| 6 | [librsvg / rsvg-convert](https://gitlab.gnome.org/GNOME/librsvg) | LGPL-2.1-or-later | Rust, 412 (mirror), active | Static SVG to PNG/PDF/PS; no SMIL | optional binary |
| 7 | [ImageMagick](https://github.com/ImageMagick/ImageMagick) | ImageMagick License (Apache-2.0 derived) | C, 17.9k, active | `-coalesce` GIF frame extraction, GIF/APNG assembly; static SVG via delegate | optional binary |
| 8 | [gifsicle](https://github.com/kohler/gifsicle) | GPL-2.0 | C, 4.4k, active | `--explode`, `--unoptimize`, `--optimize`, `--lossy`, delay and palette edits | optional binary |
| 9 | [gifski](https://github.com/ImageOptim/gifski) | AGPL-3.0 | Rust, 5.7k, active | PNG sequence to GIF, temporal dithering, best available palette quality | optional binary (AGPL) |
| 10 | [apngasm](https://github.com/apngasm/apngasm) | Zlib | C++, 355, stale | PNG frames to/from APNG | optional binary (Pillow covers) |
| 11 | [libwebp](https://github.com/webmproject/libwebp) | BSD-3-Clause | C, 2.3k, active | `gif2webp`, `img2webp`, `webpmux` | optional binary |
| 12 | [potrace](https://potrace.sourceforge.net) ([mirror](https://github.com/skyrpex/potrace)) | GPL-2.0 | C, 370, old | 1-bit bitmap to SVG/PDF; requires thresholding | reference |
| 13 | [autotrace](https://github.com/autotrace/autotrace) | GPL-2.0 | C, 754, active | Color and centerline bitmap trace to SVG | reference |
| 14 | [Inkscape CLI](https://gitlab.com/inkscape/inkscape) | GPL-3.0-or-later | C++, 4.0k, active | Static export only; no animation timeline, renders one state | reference |
| 15 | [FrameSVG](https://github.com/Romelium/FrameSVG) | MIT | Py, 28, low activity | GIF to SMIL SVG, vtracer applied per frame | reference |
| 16 | [unbake](https://github.com/FredBourouliou/unbake) | MIT | Py, new | GIF to Lottie/animated SVG by shape and motion recovery | reference |
| 17 | [svg2gifpy](https://github.com/ishandutta2007/svg2gif) | MIT | Py, 5, low activity | Playwright plus Pillow SVG to GIF CLI | reference |
| 18 | [python-lottie](https://gitlab.com/mattbas/python-lottie) | AGPL-3.0+ | Py | Lottie/SVG/GIF/WebP conversion via `lottie_convert.py` | reference (AGPL) |
| 19 | [lottie-web](https://github.com/airbnb/lottie-web) | MIT | JS, 32.4k, active | Renders Lottie JSON in browser; capture via Playwright | reference runtime |
| 20 | [GSAP](https://github.com/greensock/GSAP) / [anime.js](https://github.com/juliangarnier/anime) | GSAP free standard licence since 3.13 (non-OSI) / MIT | JS, 30.6k and 74.7k, active | Timeline tween including MorphSVG; anime.js path morphing; capture via Playwright | reference |
| 21 | [flubber](https://github.com/veltman/flubber) | MIT | JS, unmaintained since 2022 | Path interpolation, polygon approximation | reference |
| 22 | [sharp](https://github.com/lovell/sharp) | Apache-2.0 | JS, active | libvips animated GIF/WebP transcode | reference |

## Conversion matrix

| Direction | Status | Approach |
|---|---|---|
| Text/ASCII frames to GIF | Feasible now | Pillow, existing path |
| Image folder to GIF | Feasible now | Pillow; gifski optional for palette quality |
| Image folder to PNG sequence | Feasible now | Pillow normalize and copy |
| Image folder to APNG / WebP | Feasible now | Pillow `save_all` |
| Static SVG to PNG | Feasible now | Playwright; resvg or CairoSVG as offline fallback |
| Static SVG to GIF | Feasible now | One frame, nothing to animate |
| Animated SVG (SMIL/CSS/WAAPI) to PNG sequence | Feasible now | `pauseAnimations` plus `setCurrentTime`, or `getAnimations().currentTime` |
| Animated SVG to GIF | Feasible now | Above plus Pillow; partially feasible for JS/canvas/external assets |
| Animated SVG to APNG | Feasible now | Same frames plus Pillow |
| Multiple SVG files to animation | Feasible now (sequence), partially (tween) | HTML timeline plus Playwright; tween needs flubber or GSAP MorphSVG |
| Multiple SVG files to one animated SVG | Partially feasible | Hand-built SMIL `<set>`/`<animate>` timeline; no general tool |
| GIF to PNG sequence or folder | Feasible now | Pillow `seek` (composites disposal); `gifsicle --explode` fallback |
| GIF to SVG wrapper with embedded raster | Feasible now | Base64 `<image>`; no vector gain |
| GIF to traced SVG | Partially feasible | FrameSVG or vtracer per frame, flat art only |
| PNG/raster to traced SVG | Partially feasible | vtracer color, potrace 1-bit; logos and pixel art only |
| GIF to APNG / WebP | Feasible now | Pillow; `gif2webp` optional |
| GIF or APNG to WebM/MP4 | Not feasible | Requires ffmpeg, deliberately removed; use APNG/WebP |
| Lottie to GIF | Partially feasible | lottie-web plus Playwright; python-lottie is AGPL |
| GIF to Lottie | Partially feasible | unbake only, flat motion graphics |

## Pitfalls

**GIF disposal and compositing.** GIF frames are deltas against the logical screen. Pillow's `seek` returns the composited canvas, so naive extraction without compositing produces fragments and holes. `gifsicle --explode` returns raw sub-rectangles and needs `--unoptimize` first.

**Palette and transparency.** GIF is 8-bit indexed with 1-bit alpha. Any RGBA frame written to GIF is quantized to 256 colors per frame; `optimize=True` marks unchanged pixels transparent, so decode after encode is never pixel-identical. Pillow switches later frames from P to RGB when local palettes differ, so palette identity does not survive a round trip.

**Alpha.** Semi-transparent shadows and anti-aliased edges must be matted onto a background or thresholded. Traced SVG approximates alpha as `fill-opacity`; GIF edges become hard pixels or a colored halo.

**Frame timing.** GIF stores delays in 10 ms units. Chromium and Firefox clamp delays of 10 ms or less to 100 ms; 20 ms (50 fps) is the practical floor. Pillow accepts millisecond durations but rounds on write.

**SMIL sampling.** `document.getAnimations()` covers CSS animations, transitions and WAAPI only, not SMIL. Playwright `animations: 'disabled'` does not stop SMIL (microsoft/playwright#31403). Use `svg.pauseAnimations()` then `svg.setCurrentTime(t)` on the root element.

**CSS/WAAPI sampling.** `page.clock` does not advance the animation timeline; pause each animation and set `currentTime` for determinism. Wall-clock waiting produced 5-7 distinct images from 12 captures in a published measurement.

**Rasterizer divergence.** resvg, librsvg and CairoSVG have no animation and differ on filters, masks, `mix-blend-mode`, `foreignObject`, text shaping and font fallback. resvg has no native text rendering by design. Only Chromium matches what a browser shows.

**Vectorizing animation.** Per-frame tracing re-clusters colors independently, so flat regions flicker between frames and shared palettes are lost. Output scales as frames times paths; FrameSVG's own guidance is speckle filtering, polygon mode and reduced color precision to contain size.

**Lossy round trips.** GIF to PNG to GIF re-quantizes. GIF to SVG to GIF adds trace error and then quantizes again. Each cycle compounds banding and edge noise.

## Recommendation

Use a PNG sequence plus per-frame delays in milliseconds and a loop count as the single canonical intermediate. Every direction in the matrix becomes an encode step from that intermediate, and all decoding paths converge on it. Rasterize SVG through Chromium only; add resvg as an optional fast path for static SVG. Treat traced SVG as a terminal export with an explicit warning, never as an intermediate. Keep gifsicle and gifski as optional subprocess binaries gated on availability, and keep the GPL and AGPL tools outside the process boundary.
