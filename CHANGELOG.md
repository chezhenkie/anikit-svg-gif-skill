# Changelog

## 0.8.2

### Changes

* The skill bundle ships the current skill and reference text; the 0.8.1 bundles carried an earlier copy. A rest is a supported end pose held at least 3 frame times at the seam, an apex or any mid-flight pose is never a rest, and a contact holds one frame.
* The page route notes state that JavaScript in a page runs in the browser without Node, and that only a seekable CSS, WAAPI or SMIL timeline is captured; a script that writes positions every frame has none.
* The smoke run renders the wall, ceiling and ramp contact scenes and checks their frame counts, so a timeline that no longer reproduces a contact fails the check.
* The repository hygiene test matches credential prefixes only at a token start, so a name like `task-small` no longer trips it.

## 0.8.1

### Changes

* The release ships a skill bundle for each platform: one zip holding `SKILL.md`, `INSTALL.md`, the references and the matching binary, so the install is a single extract into the skills folder. The loose binaries stay on the release page as the manual route.
* `tools/make_bundle.py` builds the five bundles in the release workflow from the checked-in skill files, and the release notes name the bundle and the manual route.
* The README leads its install chapter with the bundle and keeps the manual route below it, as a checkout plus an asset download with the two steps stated separately.
* The skill is named anikit in `SKILL.md` and in the installed folder; the README intro, the repository About and the voice rule are written for a reader who has not seen the spec.
* The bin README points at the bundle and its download example is on the current tag.

## 0.8.0

### Changes

* Eased and spring values pass their end key. `Lerp::lerp` and its f32, f64 and Transform overrides extrapolate outside 0 to 1, so a curve or spring overshoot reaches the frame; the 0.7.0 build clamped the sample at the key. Two tests pin it.
* The motion references gain three rules: a rest inside a loop holds for at least 3 frame times, bounce heights and intervals decay geometrically, and squash flattens the contact face along the normal on a floor, wall, ceiling or angled surface. The rule applies to any shape.
* The contact-face diagram ships in `agent-skill/references/contact-face.png` and is named by the motion reference, the skill file list and the delivery checklist.
* The skill states its token budget: the motion values live in tables, one animation is meant to finish in one session, and the agent reads the references once and composes from the presets.
* The README leads with what the tool does, the built-in animation know-how and the path for small local models, and gains a Not implemented yet chapter for friction, squash, stretch, smear, gain, chain delay, contact times, the overshoot cap, path and raster keys, masks, particles and meshes.
* The format reference drops its Python sample, its transparent-GIF line and its APNG alpha claim; the suite text names the memory checks, the requirements name Chrome 146 and the platform linker, and the text-versus-size advice is consistent.

## 0.7.0

### Changes

* Frame plans are built on cumulative time: the exact frame times `i * 1000 / fps` are snapped to the 10 ms grid and differenced, so a 12 fps plan alternates 80 and 90 ms and a 12-frame plan totals 1000 ms. The ASCII route uses the same rule, and the seam no longer depends on a rate that divides the duration.
* The SVG export writes an explicit `100%` key that holds the last sample to the end, composes the timeline matrix left of each element's authored transform, and fails on a target id that is absent from the drawing.
* A pingpong scene doubles its plan and its duration and keeps `infinite`, so the plan contains the mirrored half instead of relying on an alternating repeat.
* The vector route captures at the resolved size times `--dpr`, so `--dpr 2` doubles both sides.
* Import accepts fps 1, 2, 4, 5, 10, 20, 25 and 50, samples `animateMotion` through the element's current matrix, and reports `animateColor` as skipped because Chromium does not run it.
* Every route checks the canvas (8 to 8192 px) and the frame count (up to 100000) before rendering. Extract checks the source header before allocating and refuses an APNG with more frame controls than its animation control declares.
* The images route streams its sources one frame at a time, keeps at most 12 palette samples, and holds only the pair a crossfade blends, so peak memory follows the canvas rather than the input length.
* The page never gets local file access, so a script in the drawing cannot read another local file. The browser starts with its own sandbox and is restarted with `--no-sandbox` only when it exits before opening its port. Temporary profile, page and log paths are drawn at random and created exclusively, and a Chromium based browser found on `PATH` under the Chromium, Chrome, Brave or Edge name is used.
* GIF readback requires the trailer. A zero delay is refused, the APNG writer accepts the longest delay the field holds, removes its temp file when setup fails, and the time sums saturate or fail rather than wrap.
* `--preview` cannot replace the output file, `--colors` is checked on every route, and a comma colour accepts spaces around its parts.
* `anikit --licences` prints the third-party notices, which are also in `THIRD-PARTY-NOTICES.txt` and embedded in every binary.
* The tween parser refuses `friction`, `squash` and `stretch` as not implemented, refuses a rate that cannot map the scene duration, and refuses a `contactHold` that overflows the plan.
* A browser test fails when the host has no browser; `ANIKIT_SKIP_BROWSER_TESTS=1` turns those tests into a printed skip. CI runs clippy and the tests on the Windows arm job as well.
* The ten samples are regenerated with the new timing; the dune triptych runs 5.36 s.

## 0.6.0

### Changes

* Tween path pairs whose subpath counts and closed flags match but whose curve counts differ resample by arc length instead of failing. Each subpath pair is sampled into a common polyline, a closed ring searches every start offset, the whole path reverses or not as one decision, and prepare reports `PathFidelity::Exact` or `Resampled { points }`. A subpath count or closed-flag mismatch still fails, and no route draws path tracks yet.
* The fallback samples at a 4 px chord clamped to 32 and 1024 points and writes a straight cubic per chord from a warm buffer, so a matched curve count keeps the exact cubic morph. Part 06 and decision row 27 record the rule.
* The sample block is rebuilt: a dune triptych for the image route, an authored pulse monitor for the vector route and an orrery for the tween route, all at 480x270, with loop seams fixed by choosing rates that divide the duration.
* Four kinetic studies join the samples as white line work on black: a ball drop with decaying bounces, a Newton's cradle, a leap with a lagging antenna and a damped travelling wave, all generated from JSON timelines with computed physics.
* `tools/make_docs.py` writes ten samples and needs a browser. The README and `docs/index.md` carry the same 2x2 sample block.
* CI uses the Node 24 majors of checkout, upload-artifact and download-artifact. This release is the first to exercise the upload and download pair.
* `REQUIREMENTS.md` records the hosts, per-route browser needs, input formats, build requirements, disk and memory, and limits. The README leads with the animation engine and the JSON timeline.

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
