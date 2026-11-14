# Animated GIF considerations

Format constraints and the defaults the tools in this skill are built on. Numbers are specifics of GIF89a and common viewers, not preferences.

## Format constraints

- GIF is 8-bit indexed: at most 256 colors per frame. Truecolor and alpha gradients are impossible without banding.
- Delays are stored in centiseconds. Viewers round or clamp small values; Chrome maps 10 ms or less to 100 ms. Keep delays at 20 ms or more.
- Disposal mode 1 (do not dispose) is correct when every frame repaints the full canvas. Partial frames need disposal 2 and tight rectangles, which complicates palette handling for little gain on flat art.
- `loop=0` loops forever. Other values repeat that many times.
- One file can mix per-frame delays, dimensions and local palettes, but uniform size and a shared palette are what prevent jitter.

## Timing

- Precompute animation state on a fixed grid. Never drive frames from a wall clock; output must be deterministic and reproducible.
- Merge consecutive identical states into one frame with a longer delay. Holds and duplicated frames cost nothing visually and shrink the file.
- Snap delays to the 10 ms grid through cumulative time, then emit differences, so rounding error cannot accumulate:

```python
def scaled_durations(runs, speed):
    out, elapsed, emitted = [], 0, 0
    for _state, ms in runs:
        elapsed += ms
        target = round(elapsed / speed / 10) * 10
        out.append(max(20, target - emitted))
        emitted += out[-1]
    return out
```

- A speed factor scales delays, not the timeline. Resampling would duplicate or drop states.
- The loop must contain whole cycles. A seam that jumps in position, color or brightness is the most visible defect in a looping GIF.
- Frame count is the main cost driver. Halve fps before halving dimensions when text legibility matters.

## Palette

- Build one palette for all frames from frames sampled across the loop, using median cut. A palette taken from one frame misses colors that appear later and causes flicker.
- Reserve one index when transparency is needed: 255 colors, index 0 transparent.
- `dither=NONE` for flat art, text and diagrams. Dithering adds noise, breaks glyph edges and increases size.
- Floyd-Steinberg dithering helps photographic gradients and skin tones. It also makes the GIF larger and grainier on flat regions.
- Quantization can shift exact brand colors. Verify the rendered output, not the source, when color accuracy matters.

## Frames and determinism

- Render every frame the same way, with the same font, viewport and background. Two runs over the same input must produce identical files.
- Write to a temp file, read it back, verify frame count, duration sum and pixel size, then rename. A printed path should always point to a complete GIF.
- Save a still preview of one frame. Palette problems and layout errors are visible without playing the animation.

## ASCII

- Use a monospace font and a fixed advance: `x = col * advance`, `y = row * line_height`. Centering each glyph individually breaks the grid.
- Line height comes from font ascent plus descent, not from the point size alone.
- Verify glyph coverage for box drawing, block elements and braille. Missing glyphs render as boxes or nothing.
- Parse ANSI SGR before layout: basic colors, bright colors, 256-color and truecolor foregrounds. A reset returns to the default color.
- Expand tabs to spaces and pad all frames to one grid; differing frame sizes are not allowed.
- Keep grids modest, around 120x40 cells. A 200x80 grid at 28 px glyphs is a very large canvas.

## Image folders

- Sort files naturally, so `pic-2` comes before `pic-10`.
- Apply EXIF orientation before cropping. Phone photos are routinely sideways without it.
- Fit every image to one canvas: cover crops, contain pads on a background color. Mixing sizes is not possible in one GIF frame sequence.
- Ken Burns zoom needs a source larger than the target. Build a 2x base once, crop a window per frame and resize down; upscaling the target itself loses detail.
- Holds, transitions and the loop seam all consume frames. A five-image reel at 12 fps with 1.2 s holds and 0.5 s fades is about 200 frames.
- Hundreds of photos at 800 px will exhaust memory and produce an unusable file. Cut count, width or fps first.

## SVG and HTML

- SVG needs a rasterizer. This skill uses Chromium through Playwright because it handles CSS, SMIL, filters, text and web fonts consistently.
- Pause animations, then seek each frame to an explicit time. Web Animations exposes `currentTime`; SMIL exposes `setCurrentTime`. Screenshots taken against real time are not deterministic.
- Match the viewport to the drawing, or set it explicitly. The page wrapper fits the SVG to the viewport and keeps its aspect ratio.
- Relative images, fonts and CSS in an SVG resolve against its own directory through a base URL.
- Preset transforms need a transform origin at the center and `transform-box: fill-box` inside SVG. Rotating the root can clip corners at the viewport edge.
- `devicePixelRatio` 2 doubles captured pixels for crisper text; the GIF is twice as large.
- JS-driven canvas and WebGL pages are outside this tool. They need a frame hook that renders at a requested time, or pre-rendered frames.
- External web fonts and network assets make capture slow and non-deterministic. Inline them when possible.

## Scaling and size

- Render at the target size. If downscaling, resize each frame with LANCZOS; resizing the finished GIF does not work.
- Keep ASCII cells at least 8x16 px so glyph strokes survive quantization.
- Reduce dimensions before frame count when text must stay sharp. Reduce fps before dimensions when motion is the point.
- Check the output size against the host. Chat and README embeds often cap around 5-10 MB.

## Alternatives

- APNG: truecolor, alpha, precise timing, browser support. Larger files, patchy support in chat apps.
- WebM or MP4: best for long, high-fps, truecolor sequences. Needs codec tooling and is not an image.
- GIF remains the most portable option for READMEs, chat and email.

## Validation checklist

- Frame count and duration sum match intent.
- Dimensions and file size are acceptable for the host.
- The loop seam has no jump in position, color or brightness.
- Colors match the source after quantization; gradients do not band visibly.
- Two runs over the same input produce identical bytes.
- The GIF plays in a browser or image viewer, not only in the generating library.
