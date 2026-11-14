---
name: animated-gif
description: "Build animated GIFs from ASCII text frames, a folder of images, an SVG or HTML animation, or a video clip, with palette, timing, looping, scaling and validation handled. Use when asked to make, convert, resize, slow down or fix an animated GIF, animate ASCII art, turn photos into a slideshow GIF, animate an SVG, or convert a video to GIF."
license: MIT
metadata:
  version: "1.0"
---

# Animated GIF

Four tools, one assembly core. Pick the route that matches the input.

| Input | Tool | Motion |
|---|---|---|
| Text frames with ANSI colors | `bin/ascii_gif.py` | frame sequence |
| Folder of photos or images | `bin/pics_gif.py` | holds, crossfade, Ken Burns |
| SVG or HTML animation | `bin/svg_gif.py` | SMIL, CSS, or preset rotate/pulse/fade |
| Video clip | `bin/video_gif.py` | sampled at a fixed rate |

All routes share `bin/gifkit.py`: one palette across the loop, 10 ms timing grid, identical consecutive frames merged into longer delays, atomic write, read-back verification.

## Dependencies

Pillow is required. Video needs ffmpeg on PATH. SVG and HTML need Node with Playwright and a Chromium headless shell.

```powershell
python -c "import PIL; print(PIL.__version__)"
ffmpeg -version
node -e "require('playwright')"
npx playwright install chromium --only-shell
```

Every tool prints one `FAIL ...` line on stderr and exits 1 when a dependency or input is missing. Do not retry with the same arguments.

## Workflow

1. Choose the route from the table. Read the matching section below.
2. Run the tool with an explicit output path inside the project being worked on. Do not write to a system temp directory or the Desktop.
3. Keep the printed summary: frame count, duration, pixel size. A printed path is a verified GIF.
4. For visual work, open the GIF or write `--preview preview.png` and inspect that still before reporting.
5. Report the absolute path plus settings that matter: fps, size, duration, mode.

## ASCII route

Input is a text file. Frames are separated by a line containing only `---`; an optional delay in ms follows the separator: `--- 250`. ANSI SGR colors are honored: 30-37, 90-97, `38;5;n` and `38;2;r;g;b`. A directory of `.txt` files is read as one frame per file, sorted by name.

```powershell
python bin/ascii_gif.py frames.txt -o project/loader.gif --size 28 --delay 120
python bin/ascii_gif.py frames/ -o project/loader.gif --preview project/loader.png
```

- `--font` must be a monospace TTF or TTC; common monospace fonts are auto-detected when it is omitted.
- `--fg` and `--bg` default to light text on a near-black canvas.
- `--delay` is the default frame delay; separator delays override it per frame.
- Keep frames within about 120 columns and 40 rows; larger canvases make long GIFs.

## Image folder route

Files are sorted naturally (`pic-2` before `pic-10`), EXIF-rotated, and fitted to one canvas.

```powershell
python bin/pics_gif.py photos/ -o project/reel.gif --mode kenburns --fps 12 --hold 1.2 --transition 0.5 --width 800
python bin/pics_gif.py photos/ -o project/reel.gif --mode sequence --fps 10 --hold 0.8 --fit contain --bg "#101010"
```

- `sequence` holds each image; `crossfade` blends between images; `kenburns` slowly zooms each image, alternating direction.
- `--width` sets the canvas; `--height` defaults to the first image aspect.
- `--fit cover` crops to fill, `contain` pads with `--bg`.
- `--hold` is seconds per image, `--transition` seconds of blend, `--zoom` the Ken Burns end zoom.
- A folder of 100 photos at 800 px and 12 fps produces a large file; cut width or fps first.

## SVG and HTML route

The tool wraps `.svg` in a page at the requested viewport, or opens `.html` directly, then pauses and seeks every animation to explicit times before each screenshot. Frames do not depend on wall-clock timing.

```powershell
python bin/svg_gif.py logo.svg -o project/logo.gif --duration 3 --fps 15
python bin/svg_gif.py logo.svg -o project/logo.gif --animate rotate --selector "#mark" --duration 2 --background "#0b0d12"
python bin/svg_gif.py page.html -o project/page.gif --width 960 --height 540 --duration 4 --fps 12
```

- Without `--animate`, the SVG or page must already animate through SMIL or CSS. Static drawings need a preset.
- Presets: `rotate`, `pulse`, `fade`, applied to the svg root or to `--selector`.
- `--width` and `--height` default to the SVG `width`/`height` or `viewBox`.
- `--dpr 2` captures at twice the viewport, for sharper text; the GIF is then twice the pixel size.
- JS-driven canvas or WebGL pages are not driven by this tool. Use a page that exposes a frame hook, or pre-render frames and assemble them with gifkit.

## Video route

ffmpeg extracts PNG frames, then gifkit builds the palette and writes the GIF. Nothing is left in temp.

```powershell
python bin/video_gif.py clip.mp4 -o project/clip.gif --fps 12 --width 800 --start 2 --duration 3
```

- `--fps` is the GIF rate, not the source rate. 10-15 is usually enough.
- `--width` scales the source; height follows the aspect and stays even.
- `--start` and `--duration` trim in seconds. Without `--duration`, the whole remaining clip is converted.
- A 30 s clip at 12 fps is 360 frames; trim first.

## Shared options

- `--colors` up to 255. Lower it only when the palette is simple or the file must shrink.
- `--dither none` is correct for flat art and text; `floyd` helps gradients and photos.
- `--loop 0` loops forever; any other value repeats that many times.
- `--preview PATH` writes a PNG still of the first frame.
- Output is deterministic for the same input, settings and toolchain.

## Custom frames

Any frame sequence can use the same core. Frames must be RGB and the same size; delays are in ms on the 10 ms grid.

```python
import sys
sys.path.insert(0, "bin")
import gifkit
from PIL import Image

frames = [Image.new("RGB", (320, 180), (n * 20, 40, 90)) for n in range(12)]
gifkit.write_gif(frames, "project/gradient.gif", [100] * 12)
```

`write_gif` builds one palette, merges identical consecutive frames, writes atomically and verifies the result. Use this route for data animations, generated diagrams or any frames the four tools do not cover.

## Contract

- The tool writes to `<name>.tmp.gif` in the output directory, verifies frame count, duration sum and pixel size by reading the file back, then renames it. A failure removes the temp file and leaves no partial target.
- The GIF uses one palette for every frame, so colors cannot flicker.
- Identical consecutive frames are merged into one frame with a longer delay; the printed frame count is the stored count.
- Delays sit on the 10 ms GIF grid and are at least 20 ms.

## Verification

Route checks live in the source repository. From the repository root:

```sh
python tools/check.py
```

Each check prints one `PASS`, `SKIP` or `FAIL` line. Missing optional dependencies skip by default; `--strict` requires every route. The checks generate fixtures under `scratch/checks` and also scan the repository for secrets, absolute paths, non-ASCII text and CRLF endings.

## Limits

- GIF is 8-bit indexed and caps at 256 colors; photos show banding without dithering.
- Viewers clamp short delays; keep frame delays at 20 ms or more.
- Long, high-fps, truecolor sequences are better as APNG or WebM. Say so when asked for quality.
- Memory grows with frame count and canvas size; the tools hold all frames while building one palette.
- ASCII glyph coverage depends on the chosen font; missing box or block glyphs fall back to empty boxes.

See [references/animated-gif-considerations.md](references/animated-gif-considerations.md) for format constraints and the reasoning behind these defaults.

## Files

- `bin/gifkit.py` - palette, timing, merge, atomic write, verification.
- `bin/ascii_gif.py` - ANSI text frames to GIF.
- `bin/pics_gif.py` - image folder to GIF.
- `bin/svg_gif.py` - SVG/HTML animation to GIF.
- `bin/render_frames.cjs` - Playwright frame capture helper.
- `bin/video_gif.py` - video to GIF through ffmpeg.
- `references/animated-gif-considerations.md` - format notes.

Repository checks and sample generation are in `tools/`, outside the installed skill payload.
