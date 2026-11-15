# 02-crate-split

Status: agreed direction, not implemented. Package naming is open in 09, row 7; the crate boundaries below do not depend on it.

## 2.1 Crates

Seven crates, one binary.

| crate | kind | owns |
|---|---|---|
| core | lib | frame buffer and pixel format, palette rules, quantisation interface, GIF writer, readback verification, the error type |
| time | lib | presentation times, the 10 ms grid, cumulative rounding, the frame plan, loop modes |
| tween | lib | timeline, tracks, keys, easing, property values, layers and composition |
| render-text | lib | glyph grid, ANSI SGR parsing, font discovery and metrics |
| render-raster | lib | image decoding, EXIF rotation, fit, Ken Burns, crossfade, pixel adjustments |
| render-svg | lib | browser discovery, page construction, seek and capture, preset animation |
| cli | bin | argument parsing, route dispatch, messages, exit codes, preview output |

There is no render-web crate. HTML and JavaScript pages are out of scope for the port, so the vector route handles SVG plus pages whose animation is CSS, WAAPI or SMIL.

## 2.2 Dependency direction

```text
cli           -> render-text, render-raster, render-svg, tween, time, core
render-text   -> tween, time, core
render-raster -> tween, time, core
render-svg    -> tween, time, core
tween         -> time, core
time          -> core
core          -> none
```

One way only. `core` and `time` sit at the bottom, `cli` on top, and no renderer depends on another renderer. A renderer that needs a rule from a sibling has found a rule that belongs in `core` or `tween`.

## 2.3 What each boundary owns

- `core` owns bytes and files: the frame buffer type, palette construction and application, the writer, and verification. It knows nothing about fonts, SVG, decoding or animation.
- `time` owns the mapping from a duration and a frame rate to presentation times, including the 10 ms grid and the cumulative rounding rule. It holds no pixels and no property values.
- `tween` owns values over time. It knows nothing about pixels, files or fonts, and it never allocates per sample.
- `render-text` owns glyphs, font metrics and colour per cell. It does not quantise or write.
- `render-raster` owns decoding, geometry and per-pixel adjustment. It does not choose a palette.
- `render-svg` owns the browser: finding it, driving it, and turning a time into an image. It does not compose frames.
- `cli` is the composition root: it validates arguments, builds a plan, selects a renderer, and reports. No route rules live there.

## 2.4 Binary and subcommands

One binary, because the three routes share the core and a second executable would duplicate the writer and the error contract. The route is selected by a subcommand that matches the current script name, so `ascii`, `images` and `vector` replace `ascii_gif.py`, `pics_gif.py` and `svg_gif.py`.

The package and binary names are open in 09, row 7. The skill's invocation table in README.md is updated in the same change that introduces the binary.
