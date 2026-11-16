# Changelog

## 0.5.0

### Changes

* The tween route switches a target's own CSS, WAAPI or SMIL animation off before the page loads and cancels any remaining animation object, so a scene over an animated drawing renders each motion once. A scene imported from an animated SVG previously applied the source animation on top of the timeline.
* An imported scene renders at its sampled values: the demo drawing stores 29 frames against the source's 28, where the earlier build stored 41 with the dots up to 56 pixels off.
* The skill states the rule, and part 11, part 14 and decision row 26 record it.

## 0.4.0

### New features

#### All platforms

* Fifth route, `anikit extract`: a GIF or APNG is split into `frame-0001.png` full-canvas frames and a `manifest.json` with the size, loop count and per-frame delay.
* Sixth route, `anikit import`: an animated SVG, CSS or SMIL, is sampled into the timeline JSON at the frame times, so a drawing can be re-rendered by the tween route or edited as data.
* `anikit images --timing`: a frame folder and its manifest are rebuilt into a GIF or APNG at the recorded delays and loop count, with no fps grid and no transition.

### Changes

* Extract reads a GIF through `image` and an APNG through `png`, honours source and over blending and the three dispose operations, then reads every frame and the manifest back before renaming the folder.
* Import loads one page, collects animated elements from the Web Animations API and from SMIL elements, and samples transform, opacity, fill, stroke and stroke width. A still drawing, a missing element id, a non-zero CSS transform origin or a root `svg` target is refused before the scene is written, and an unsupported property is skipped with a WARN line.
* `anikit-render-raster` owns extract and the timed image rebuild; `anikit-render-svg` owns import.
* A browser launch is retried once with a fresh profile after 500 ms, and browser tests take a per-binary lock so parallel test binaries do not race for the port.
* The skill documents all six routes, the two file formats extract reads and the options import accepts.

## 0.3.1

### New features

#### Windows arm

* `anikit-windows-arm.exe`, built on the Windows arm runner.

### Changes

* CI builds the Windows arm binary and runs the text route to GIF and APNG and the tween route to SVG from it.
* The release matrix carries five assets: Linux x86_64, Linux arm, Windows x86_64, Windows arm and macOS arm.

## 0.3.0

### New features

#### All platforms

* Fourth route, `anikit tween`: a static SVG drawing is moved by a JSON timeline of absolute times. The tween crate supplies keyframes, easing, springs, arcs, ballistic motion and layers; transform, opacity, fill, stroke and stroke width are written per frame through one browser page.
* APNG output on every route, chosen by a `.png` or `.apng` extension. Frames stay truecolor, the palette pass is skipped, and the timing and merge rules are the GIF ones.
* SVG export on the tween route, chosen by a `.svg` extension. The prepared plan is written as CSS keyframes inside the drawing; it needs no browser and no script.
* Release assets for Linux x86_64, Linux arm (static musl), macOS arm and Windows x86_64.

### Changes

* The tween route is the first consumer of `anikit-tween`; no new crate was added.
* `anikit-render-svg` exposes one page and capture path, shared by the vector and tween routes, and owns the SVG export.
* `anikit-core` carries `png` for APNG frame compression; it is the only dependency in core.
* CI builds the arm Linux and macOS arm targets; the arm job also runs the text and images routes from the musl binary.
* The skill documents the tween route, its timeline grammar, the three output formats and the browser requirement that covers the frame outputs of both browser routes.

## 0.2.0

### New features

#### All platforms

* One binary, `anikit`, with the three routes as subcommands: `ascii`, `images` and `vector`.
* Frames stream through the encoder, so memory follows the canvas rather than the frame count; a 1000-frame run stays within 2 MiB of a 100-frame run.
* Every frame after the first is written as the rectangle that changed, with disposal 1.
* One palette per loop, sampled from up to 12 frames, with flat colours kept exactly.
* The text route draws with an embedded DejaVu Sans Mono face, so the grid does not depend on the host's fonts. `--font` or `ANIM_GIF_FONT` replaces it.
* The image route reads png, jpeg, gif, webp, bmp and tiff, applies EXIF orientation, expands animated sources to all their frames, and mattes transparency onto `--bg`.
* The vector route drives an installed Chromium based browser over one DevTools session, seeking every animation to explicit times.
* Release assets for Linux x86_64 (static musl) and Windows x86_64, built by the release workflow on the version tag.

### Changes

* The skill installs the binary from a release asset and no longer needs Python, Pillow, Node or Playwright.
* The Python payload (`ascii_gif.py`, `pics_gif.py`, `svg_gif.py`, `gifkit.py`, `render_frames.cjs`) and `tools/check.py` are deleted. The payload stays at tag `python-freeze` as a reference for behaviour.
* Route checks are the Rust test suite and CI.
* A custom frame sequence is a folder of PNGs through `images --mode sequence`; per-frame delays are no longer accepted.
* The sample GIFs are regenerated by `tools/make_docs.py` driving the binary.

## 0.1.1

### Fixes

* Corrected the route count in the custom frames section.

## 0.1.0

### New features

#### All platforms

* Three routes: ASCII text frames, image folder, SVG or HTML animation.
* Shared core with one palette per loop, a 10 ms timing grid, identical-frame merging and atomic verified writes.
* Fixed-canvas option for ASCII frames, so samples and badges share one size.
* Route checks with optional dependency skips and repository hygiene scanning.
* Sample generator for the README.
