---
name: animated-gif
description: "Build animated GIFs and APNGs from ASCII text frames, a folder of images, an SVG or HTML animation, or a JSON timeline over a static SVG drawing, export a timeline as an animated SVG, import an animated SVG into that timeline, and split a GIF or APNG into frames with their timing, with palette, timing, looping, scaling and validation handled. Use when asked to make, convert, resize, slow down or fix an animated GIF, produce an APNG, animate ASCII art, turn photos into a slideshow GIF, animate an SVG, keyframe an SVG drawing, or extract frames and delays from an animation."
license: MIT
metadata:
  version: "0.4.0"
---

# anikit

The binary has six routes and one shared timing core; pick the route that matches the input.

| Input | Command | Motion |
|---|---|---|
| Text frames with ANSI colors | `bin/anikit ascii` | frame sequence |
| Folder of photos or images | `bin/anikit images` | holds, crossfade, Ken Burns |
| SVG or HTML animation | `bin/anikit vector` | SMIL, CSS, or preset rotate/pulse/fade |
| Static SVG drawing and a JSON timeline | `bin/anikit tween` | keyframes, easing, springs, ballistic motion, layers |
| Animated SVG, CSS or SMIL | `bin/anikit import` | sampled into the JSON timeline |
| Animated GIF or APNG | `bin/anikit extract` | frames and timing written to a folder |

Every frame route ends in the same core: 10 ms timing grid, identical consecutive frames merged into longer delays, atomic write, read-back verification. GIF adds one palette across the loop and delta frames; APNG is truecolor and writes full frames. The tween route can also write an animated SVG.

The output extension chooses the format: `.gif`, `.png` or `.apng` on every route, `.svg` on the tween route. An unknown extension fails before rendering.

## Binary

`bin/anikit` is one platform binary (`bin/anikit.exe` on Windows). It needs no Python, no Node and no package manager. When the host has no binary for its platform, report the platform as unsupported and stop: do not build, download or substitute one during a GIF run.

The vector and import routes, and the tween route when it writes GIF or APNG, also need an installed Chromium based browser. An SVG export does not. `GIFKIT_BROWSER` or `--browser PATH` names one explicitly. Without either, Chromium, Google Chrome and Edge are searched in their install locations, then on `PATH`.

A failed run prints one `FAIL ...` line on stderr and exits 1. Do not retry with the same arguments.

## Workflow

1. Choose the route from the table. Read the matching section below, and read [references/kinetic-motion.md](references/kinetic-motion.md) before setting fps, delay, easing or a preset.
2. Run the binary with an explicit output path inside the project being worked on. Do not write to a system temp directory or the Desktop.
3. Keep the printed summary: frame count, duration, pixel size. A printed path is a verified file.
4. For visual work, open the GIF or write `--preview preview.png` and inspect that still before reporting.
5. Report the absolute path plus settings that matter: fps, size, duration, mode.
6. Run the motion checklist in `references/kinetic-motion.md` before reporting. A move with no held contact, a bounce that does not decay, or a spring still moving at the loop point is a defect, not a preference.

## ASCII route

Input is a text file. Frames are separated by a line containing only `---`; an optional delay in ms follows the separator: `--- 250`. ANSI SGR colors are honored: 30-37, 90-97, `38;5;n` and `38;2;r;g;b`. Other CSI sequences are stripped. A directory of `.txt` files is read as one frame per file, sorted by name.

```powershell
bin/anikit ascii frames.txt -o project/loader.gif --size 28 --delay 120
bin/anikit ascii frames/ -o project/loader.gif --preview project/loader.png
```

- The bundled face is DejaVu Sans Mono, covering Basic Latin, box drawing and block elements. `--font` or `ANIM_GIF_FONT` replaces it with another monospace TrueType file; a missing `--font` path fails.
- `--width` and `--height` fix the canvas and center the text grid. Give both, or neither.
- `--fg` and `--bg` default to light text on a near-black canvas.
- `--delay` is the default frame delay; separator delays override it per frame.
- Keep frames within about 120 columns and 40 rows; larger canvases make long GIFs.

## Image folder route

Files are sorted naturally (`pic-2` before `pic-10`), EXIF-rotated, and fitted to one canvas. png, jpeg, gif, webp, bmp and tiff are read.

```powershell
bin/anikit images photos/ -o project/reel.gif --mode kenburns --fps 12 --hold 1.2 --transition 0.5 --width 800
bin/anikit images photos/ -o project/reel.gif --mode sequence --fps 10 --hold 0.8 --fit contain --bg "#101010"
```

- `sequence` holds each image; `crossfade` blends between images; `kenburns` slowly zooms each image, alternating direction.
- `--width` sets the canvas; `--height` defaults to the first image aspect.
- `--fit cover` crops to fill, `contain` pads with `--bg`.
- `--hold` is seconds per image, `--transition` seconds of blend, `--zoom` the Ken Burns end zoom.
- An animated GIF or WebP source contributes every frame in order; its own delays are ignored and the holds apply between frames.
- Transparency is matted onto `--bg` before quantisation, so the GIF is opaque.
- A folder of 100 photos at 800 px and 12 fps produces a large file; cut width or fps first.

## SVG and HTML route

An `.svg` is wrapped in a page at the requested viewport; an `.html` page is loaded as it is. The binary loads the page once, pauses every animation and seeks each frame to an explicit time before the screenshot, so frames do not depend on wall-clock timing.

```powershell
bin/anikit vector logo.svg -o project/logo.gif --duration 3 --fps 15
bin/anikit vector logo.svg -o project/logo.gif --animate rotate --selector "#mark" --duration 2 --background "#0b0d12"
bin/anikit vector page.html -o project/page.gif --width 960 --height 540 --duration 4 --fps 12
```

- Without `--animate`, the SVG or page must already animate through SMIL, CSS or the Web Animations API. Static drawings need a preset.
- Presets: `rotate`, `pulse`, `fade`, applied to the svg root or to `--selector`.
- `--width` and `--height` default to the SVG `width`/`height` or `viewBox`.
- `--dpr 2` captures at twice the viewport, for sharper text; the GIF is then twice the pixel size.
- A JavaScript canvas or WebGL page is not driven by this route. Use a page that animates through CSS or SMIL, or pre-render frames and use the image route.

## Tween route

A static `.svg` drawing is moved by a JSON timeline. The timeline names the canvas, the frame rate, one track per property, and optional layers. Times are absolute milliseconds and the last key sets the duration.

```json
{
  "version": 1,
  "canvas": {"w": 480, "h": 270, "bg": "#0b0d12", "fps": 25},
  "timing": {"loop": "cycle", "rate": 1.0},
  "tracks": [
    {"id": "mark-move", "kind": "svg.transform", "src": "mark.svg#mark",
     "keys": [
       {"t": 0, "translate": [0, 0], "easing": "ease-out"},
       {"t": 800, "translate": [120, 40], "easing": "hold"},
       {"t": 1000, "translate": [120, 40]}
     ]}
  ]
}
```

```powershell
bin/anikit tween scene.json -o project/scene.gif --preview project/scene.png
bin/anikit tween scene.json -o project/scene.apng
bin/anikit tween scene.json -o project/scene.svg
```

- Kinds: `svg.transform`, `svg.opacity`, `svg.fill`, `svg.stroke`, `svg.stroke-width`.
- A transform key may set `translate` `[x, y]`, `rotate` degrees, `scale` (a number or `[x, y]`) and `skew` degrees. A part missing from a key keeps the previous key's value.
- `svg.opacity`, `svg.fill` and `svg.stroke` keys carry `value`; `svg.stroke-width` carries a number. Colours are `#rgb` or `#rrggbb`.
- `easing` is `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `hold`, `{"bezier": [x1, y1, x2, y2]}`, `{"steps": n, "at": "end"}` or `{"spring": {"stiffness": 400, "damping": 30, "mass": 1, "settle": 300}}`.
- `svg.opacity` and `svg.stroke-width` keys may take `motion` instead of `easing`: `{"ballistic": {"plane": 0, "restitution": 0.5}}` or `{"arc": {"bend": 0.1, "easing": "ease-in-out"}}`. Transforms cannot use ballistic motion.
- `timing.loop` is `once`, `cycle` or `pingpong` and maps time, not the GIF repeat count. `rate` may be negative; reverse playback needs `cycle` or `pingpong`, because `once` clamps a reversed time to the start. The repeat count is still `--loop`.
- A layer adds, multiplies or replaces a track under a time window and a weight: `{"id": "shake", "order": 1, "window": [300, 700], "weight": [{"t": 0, "value": 1}, {"t": 700, "value": 0}], "bindings": [{"target": "mark-move", "op": "add", "keys": [{"t": 0, "translate": [4, 0]}]}]}`. `order` decides application; bindings take `op` (`replace`, `add`, `multiply`, `accumulate`) and, for transforms, `mode` (`append` or `prepend`).
- Every track must name the same `.svg` file, and `src` is `file.svg#elementId`. The root `svg` cannot be a target. The drawing's own CSS or SMIL animation is paused, so the timeline is the only motion.
- Canvas size, fps and duration come from the JSON, not from flags. The frame count rounds the duration up at the canvas fps.
- An SVG export needs no browser. It writes the sampled plan as CSS keyframes inside the drawing and prints `N samples` instead of frames. It replaces any authored transform or CSS animation on the properties it drives, and it keeps the drawing's own size, not the canvas size. `--preview` is refused with `.svg`.
- Path morphs, raster keys, ASCII grids and spatial masks are not supported by this route.

## Import route

`import` samples an animated SVG and writes the JSON timeline the tween route reads, so an existing CSS or SMIL animation becomes editable keyframes.

```powershell
bin/anikit import banner.svg -o project/banner.json --duration 3 --fps 25
bin/anikit tween project/banner.json -o project/banner.gif
```

- The drawing must animate through CSS, WAAPI or SMIL. Every animated element needs an id; the root `svg` cannot be the target.
- `--duration` is the sampled interval, `--fps` the scene and sample rate. The fps must divide 1000: 1, 2, 4, 5, 8, 10, 20, 25, 40 or 50. This keeps every frame on a whole millisecond and the scene exact at its own frame times.
- Only transform, opacity, fill, stroke and stroke-width are imported. Other properties are reported with `WARN`; a drawing with nothing else importable fails.
- Values come from the rendered page, so CSS and SMIL both work. The source easing is baked into the samples and every segment is linear; render the imported scene at the import fps for an exact match.
- A colour that changes must be opaque. A non-zero CSS `transform-origin` is refused; the drawing's own static `transform` is divided out, because the tween route adds its matrix outside the element.
- `--loop-mode once|cycle|pingpong` sets the scene's time map, default `cycle`. The browser is required, as on the vector route.

## Extract route

`extract` reads an existing GIF or APNG back into frames, so a file can be inspected, edited or rebuilt.

```powershell
bin/anikit extract reel.gif -o project/frames
bin/anikit images project/frames -o project/rebuilt.gif --timing project/frames/manifest.json
```

- One PNG per stored frame: `frame-0001.png` and so on, full canvas, matted onto `--bg` (`#000000` by default).
- `manifest.json` records the source size, loop count and per-frame delays in ms. Frames are not merged or snapped on the way out.
- The output folder must be new. The folder is built under a temp name, every PNG and the manifest are read back, and the folder is renamed only then.
- `images --timing` rebuilds the timeline: each frame holds for its recorded delay, with no fps grid and no transition. Missing `--width`, `--height` and `--loop` take the manifest values.
- The rebuilt file can carry fewer frames than the folder when identical frames merge; the timeline is the same.

## Shared options

- `--colors` 2 to 255 and `--dither none|floyd` apply to GIF only; APNG and SVG are truecolor and ignore both.
- `--dither none` is correct for flat art and text; `floyd` helps gradients and photos.
- `--loop 0` loops forever; any other value repeats that many times. On APNG it is the play count; an SVG export ignores it.
- `--preview PATH` writes a PNG still of the first frame. It is refused when the output is `.svg`.
- `--timing PATH` on the images route rebuilds the delays of an `extract` manifest and ignores `--mode`, `--fps`, `--hold`, `--transition` and `--zoom`.
- Output is deterministic for the same input, settings and binary.

## Custom frames

A generated sequence becomes a folder of PNGs and the image route:

```powershell
bin/anikit images frames/ -o project/plot.gif --mode sequence --fps 10 --hold 0.1 --width 480 --height 270
```

Frames must share one size and are sorted by name, so number them. `--hold` rounds to the frame grid: at `--fps 10`, `--hold 0.1` shows one frame per 100 ms. Use this route for data animations, generated diagrams or any frames the six routes above do not cover.

## Contract

- Every output is written under a temp name beside the target, verified by reading it back, then renamed. A failure removes the temp file and leaves no partial target.
- GIF uses one palette for every frame, so colors cannot flicker. APNG stores truecolor frames and builds no palette.
- Identical consecutive frames are merged into one frame with a longer delay; the printed frame count is the stored count.
- Delays sit on the 10 ms grid and are at least 20 ms.
- After the first frame, each GIF frame carries only the rectangle that changed. APNG writes full frames.
- An SVG export writes one CSS keyframe rule per property into the drawing and is read back byte for byte before the rename.

## Limits

- GIF is 8-bit indexed and caps at 256 colors; photos show banding without dithering. Write APNG when truecolor matters.
- Viewers clamp short delays; keep frame delays at 20 ms or more.
- Frames stream through the encoder, so memory follows the canvas rather than the frame count.
- Glyph coverage follows the chosen face; a codepoint the font lacks draws an empty box.

See [references/animated-gif-considerations.md](references/animated-gif-considerations.md) for format constraints and the reasoning behind these defaults.

## Files

- `bin/anikit` - the binary: route parsing, the six routes, the GIF and APNG writers, the SVG export and import.
- `references/animated-gif-considerations.md` - format notes.
- `references/kinetic-motion.md` - mandatory motion rules: spacing, contact holds, weight, bounce decay, spring and curve choice, defaults, checklist.

Source and release binaries: https://github.com/chezhenkie/anikit-svg-gif-skill
