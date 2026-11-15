---
name: animated-gif
description: "Build animated GIFs from ASCII text frames, a folder of images, or an SVG or HTML animation, with palette, timing, looping, scaling and validation handled. Use when asked to make, convert, resize, slow down or fix an animated GIF, animate ASCII art, turn photos into a slideshow GIF, or animate an SVG."
license: MIT
metadata:
  version: "0.2.0"
---

# Animated GIF

One binary, three routes, one assembly core. Pick the route that matches the input.

| Input | Command | Motion |
|---|---|---|
| Text frames with ANSI colors | `bin/anikit ascii` | frame sequence |
| Folder of photos or images | `bin/anikit images` | holds, crossfade, Ken Burns |
| SVG or HTML animation | `bin/anikit vector` | SMIL, CSS, or preset rotate/pulse/fade |

Every route ends in the same core: one palette across the loop, 10 ms timing grid, identical consecutive frames merged into longer delays, delta frames, atomic write, read-back verification.

## Binary

`bin/anikit` is one platform binary (`bin/anikit.exe` on Windows). It needs no Python, no Node and no package manager. When the host has no binary for its platform, report the platform as unsupported and stop: do not build, download or substitute one during a GIF run.

The vector route also needs an installed Chromium based browser. `GIFKIT_BROWSER` or `--browser PATH` names one explicitly. Without either, Chromium, Google Chrome and Edge are searched in their install locations, then on `PATH`.

A failed run prints one `FAIL ...` line on stderr and exits 1. Do not retry with the same arguments.

## Workflow

1. Choose the route from the table. Read the matching section below, and read [references/kinetic-motion.md](references/kinetic-motion.md) before setting fps, delay, easing or a preset.
2. Run the binary with an explicit output path inside the project being worked on. Do not write to a system temp directory or the Desktop.
3. Keep the printed summary: frame count, duration, pixel size. A printed path is a verified GIF.
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

## Shared options

- `--colors` 2 to 255. Lower it only when the palette is simple or the file must shrink.
- `--dither none` is correct for flat art and text; `floyd` helps gradients and photos.
- `--loop 0` loops forever; any other value repeats that many times.
- `--preview PATH` writes a PNG still of the first frame.
- Output is deterministic for the same input, settings and binary.

## Custom frames

A generated sequence becomes a folder of PNGs and the image route:

```powershell
bin/anikit images frames/ -o project/plot.gif --mode sequence --fps 10 --hold 0.1 --width 480 --height 270
```

Frames must share one size and are sorted by name, so number them. `--hold` rounds to the frame grid: at `--fps 10`, `--hold 0.1` shows one frame per 100 ms. Use this route for data animations, generated diagrams or any frames the three routes above do not cover.

## Contract

- The tool writes to `<name>.tmp.gif` in the output directory, verifies frame count, duration sum and pixel size by reading the file back, then renames it. A failure removes the temp file and leaves no partial target.
- The GIF uses one palette for every frame, so colors cannot flicker.
- Identical consecutive frames are merged into one frame with a longer delay; the printed frame count is the stored count.
- Delays sit on the 10 ms GIF grid and are at least 20 ms.
- After the first frame, each frame carries only the rectangle that changed.

## Limits

- GIF is 8-bit indexed and caps at 256 colors; photos show banding without dithering.
- Viewers clamp short delays; keep frame delays at 20 ms or more.
- Long, high-fps, truecolor sequences are better as APNG or WebM. Say so when asked for quality.
- Frames stream through the encoder, so memory follows the canvas rather than the frame count.
- Glyph coverage follows the chosen face; a codepoint the font lacks draws an empty box.

See [references/animated-gif-considerations.md](references/animated-gif-considerations.md) for format constraints and the reasoning behind these defaults.

## Files

- `bin/anikit` - the binary: route parsing, the three routes, the shared core.
- `references/animated-gif-considerations.md` - format notes.
- `references/kinetic-motion.md` - mandatory motion rules: spacing, contact holds, weight, bounce decay, spring and curve choice, defaults, checklist.

Source and release binaries: https://github.com/chezhenkie/anikit-svg-gif-skill
