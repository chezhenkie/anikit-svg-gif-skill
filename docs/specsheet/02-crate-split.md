# 02-crate-split

Status: implemented. Package naming is settled in 09, row 7, the browser libraries in row 19, and the png crate in row 22; the boundaries below are the ones the workspace uses.

## 2.1 Crates

Seven crates, one binary. Every package carries the `anikit-` prefix; this part and the parts after it use the short name, so `tween` means `anikit-tween`.

| package | kind | owns |
|---|---|---|
| anikit-core | lib | frame buffer and pixel format, palette rules, quantisation interface, GIF and APNG writers, readback verification, the error type |
| anikit-time | lib | presentation times, the 10 ms grid, cumulative rounding, the frame plan, loop modes |
| anikit-tween | lib | timeline, tracks, keys, easing, property values, layers and composition |
| anikit-render-text | lib | glyph grid, ANSI SGR parsing, glyph loading and metrics |
| anikit-render-raster | lib | image decoding, EXIF rotation, fit, Ken Burns, crossfade, pixel adjustments, extraction of GIF and APNG frames, the timing manifest |
| anikit-render-svg | lib | browser discovery, page construction, seek and capture, preset animation, timeline-driven element state, SVG export, and sampling an animated SVG into a timeline |
| anikit-cli | bin | argument parsing, route dispatch, messages, exit codes, preview output |

There is no render-web crate. HTML and JavaScript pages are out of scope for the port, so the vector route handles SVG plus pages whose animation is CSS, WAAPI or SMIL. The tween route accepts an SVG drawing only, because its `src` names elements.

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

One way only. `core` and `time` sit at the bottom, `cli` on top, and no renderer depends on another renderer. A renderer that needs a rule from a sibling has found a rule that belongs in `core` or `tween`. `core` carries one external crate, `png`, for APNG frame compression (09, row 22); `time` carries none. `render-raster` carries `image` and `kamadak-exif` for decoding (row 18), `png` for APNG extraction and frame writing, and `serde_json` for the extract manifest (row 24).

The same graph as a picture: docs/ascii-crate-flow.txt.

## 2.3 What each boundary owns

- `core` owns bytes and files: the frame buffer type, palette construction and application, the two frame writers, and verification. It knows nothing about fonts, SVG, decoding or animation. It carries `png` so APNG frames can be deflated, and no other dependency.
- `time` owns the mapping from a duration and a frame rate to presentation times, including the 10 ms grid and the cumulative rounding rule. It holds no pixels and no property values.
- `tween` owns values over time. It knows nothing about pixels, files or fonts, and it never allocates per sample.
- `render-text` owns glyphs, font metrics and colour per cell. It does not quantise or write.
- `render-raster` owns decoding, geometry and per-pixel adjustment, and the reverse direction of part 13: reading a GIF or APNG back into frames and a timing manifest, and rebuilding slideshow timing from that manifest. It does not choose a palette. It carries the `image` and `kamadak-exif` crates, so `time` and the text route keep no image dependencies (09, row 18), `png` for APNG frames, and `serde_json` for the manifest.
- `render-svg` owns the browser: finding it, driving it, and turning a time into an image. It also writes the SVG export of a prepared timeline, because it owns the plan and the drawing, and it samples an animated SVG into that timeline on the import route. It does not compose frames or choose a palette.
- `cli` is the composition root: it validates arguments, builds a plan, selects a renderer, and reports. No route rules live there.

## 2.4 Binary and subcommands

One binary, `anikit` (`anikit.exe` on Windows), because the routes share the core and a second executable would duplicate the writer and the error contract. The text, images and vector subcommands carry the three Python routes under the names `ascii`, `images` and `vector`; `tween`, `import` and `extract` are new in the Rust binary.

The binary is produced by the `anikit-cli` package; the other packages are libraries and ship no executable. The skill's invocation table in README.md is updated in the same change that introduces the binary.

## 2.5 Build targets

| target | artifact | notes |
|---|---|---|
| Windows x86_64 | anikit.exe | the installed skill path on Windows |
| Windows arm | anikit.exe | the installed skill path on an arm Windows host |
| Linux x86_64, glibc | anikit | glibc distributions |
| Linux x86_64, musl | anikit | static, runs on Alpine and in scratch containers |
| Linux arm, musl | anikit | static, same rules as x86_64 musl |
| macOS arm | anikit | Apple silicon, no special flags |

The text and images routes must build for every target with no system libraries beyond libc. The text route carries its own face in the binary, so a musl build and Alpine need no font package. The vector and tween routes depend on an installed Chromium; on musl the distribution package is the only source of one.

## 2.6 Delivery

Every release attaches the binaries built for the targets in 2.5 as release assets (09, row 20). Builds run in GitHub Actions on the tag, so no developer machine produces a release asset and the installed skill never downloads, unpacks or builds anything while a GIF is made. A host with no matching asset is told so plainly instead of falling back to a compiler or a package manager. The only runtime requirement outside the binary is the browser of row 19, and only the vector and tween routes use it.

The workflow builds Windows x86_64, Windows arm, Linux x86_64 static musl, Linux arm static musl and macOS arm, and attaches each under its asset name. A host with no matching asset is reported as unsupported.
