# 02-crate-split

Status: agreed direction, not implemented. Package naming is open in 09, row 7; the crate boundaries below do not depend on it.

## 2.1 Crates

Seven crates, one binary. Every package carries the `anikit-` prefix; this part and the parts after it use the short name, so `tween` means `anikit-tween`.

| package | kind | owns |
|---|---|---|
| anikit-core | lib | frame buffer and pixel format, palette rules, quantisation interface, GIF writer, readback verification, the error type |
| anikit-time | lib | presentation times, the 10 ms grid, cumulative rounding, the frame plan, loop modes |
| anikit-tween | lib | timeline, tracks, keys, easing, property values, layers and composition |
| anikit-render-text | lib | glyph grid, ANSI SGR parsing, glyph loading and metrics |
| anikit-render-raster | lib | image decoding, EXIF rotation, fit, Ken Burns, crossfade, pixel adjustments |
| anikit-render-svg | lib | browser discovery, page construction, seek and capture, preset animation |
| anikit-cli | bin | argument parsing, route dispatch, messages, exit codes, preview output |

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

The same graph as a picture: docs/ascii-crate-flow.txt.

## 2.3 What each boundary owns

- `core` owns bytes and files: the frame buffer type, palette construction and application, the writer, and verification. It knows nothing about fonts, SVG, decoding or animation.
- `time` owns the mapping from a duration and a frame rate to presentation times, including the 10 ms grid and the cumulative rounding rule. It holds no pixels and no property values.
- `tween` owns values over time. It knows nothing about pixels, files or fonts, and it never allocates per sample.
- `render-text` owns glyphs, font metrics and colour per cell. It does not quantise or write.
- `render-raster` owns decoding, geometry and per-pixel adjustment. It does not choose a palette.
- `render-svg` owns the browser: finding it, driving it, and turning a time into an image. It does not compose frames.
- `cli` is the composition root: it validates arguments, builds a plan, selects a renderer, and reports. No route rules live there.

## 2.4 Binary and subcommands

One binary, `anikit` (`anikit.exe` on Windows), because the three routes share the core and a second executable would duplicate the writer and the error contract. The route is selected by a subcommand that matches the current script name, so `anikit ascii`, `anikit images` and `anikit vector` replace `ascii_gif.py`, `pics_gif.py` and `svg_gif.py`.

The binary is produced by the `anikit-cli` package; the other packages are libraries and ship no executable. The skill's invocation table in README.md is updated in the same change that introduces the binary.

## 2.5 Build targets

| target | artifact | notes |
|---|---|---|
| x86_64-pc-windows-msvc | anikit.exe | the installed skill path on Windows |
| x86_64-unknown-linux-gnu | anikit | glibc distributions |
| x86_64-unknown-linux-musl | anikit | static, runs on Alpine and in scratch containers |
| aarch64-unknown-linux-musl | anikit | optional, same rules as x86_64 musl |
| aarch64-apple-darwin, x86_64-apple-darwin | anikit | macOS, no special flags |

The text and images routes must build for every target with no system libraries beyond libc. The text route carries its own face in the binary, so a musl build and Alpine need no font package. The vector route depends on an installed Chromium, and on musl that is the distribution package, because the Playwright build is linked against glibc and will not start.
