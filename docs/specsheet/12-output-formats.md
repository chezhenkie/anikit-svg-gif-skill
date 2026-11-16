# 12-output-formats

Status: implemented. GIF, APNG and SVG export. The format is chosen by the output extension before any work starts, and a route that cannot write the chosen format fails before it renders.

## 12.1 Choosing the format

| extension | format | routes |
|---|---|---|
| `.gif` | GIF89a | all four |
| `.png`, `.apng` | APNG | all four |
| `.svg` | SVG with CSS keyframes | tween only |

There is no default extension: the command line defaults are `.gif`, but an output path with any other ending fails with `output must end in .gif, .png, .apng or .svg`. A frame-based route that is given `.svg` fails before rendering, with `SVG output is only written by the tween route`.

The choice is made before the palette pass. GIF builds the palette from up to twelve samples; APNG and SVG never build one, so they also never dither. `--colors` and `--dither` are accepted on every route and have no effect on APNG or SVG.

The summary line is `<name>  N frames, X.XX s, W x H` for GIF and APNG. For SVG it is `<name>  N samples, X.XX s, W x H`, because the file holds sampled keyframes, not captured frames. An SVG export captures nothing, so `--preview` is refused for it.

## 12.2 APNG

APNG is truecolor RGB8 with 8 bits per channel. There is no palette, no quantisation and no transparency: every frame is the full canvas, matted before it reaches the writer like every other frame.

Timing follows the same rules as GIF. Delays are multiples of 10 ms on the cumulative presentation clock, each delay is the gap to the next frame, and a run of identical consecutive frames merges into one frame with the summed delay. One frame is buffered; no frame list is held. `--loop` becomes the play count in the animation control chunk, where 0 means forever. The merge compares the frames the format stores, palette indices for GIF and RGB pixels for APNG, so an APNG can keep a frame that GIF merges.

The file is assembled as follows:

- PNG signature, `IHDR` (8-bit truecolor, no interlace), `acTL` with a reserved frame count and the play count.
- For every merged frame, one `fcTL` with the full canvas, no offset, `delay_num` in hundredths of a second, disposal 0 (none) and blend 0 (source). The first frame's data is one `IDAT`; every later frame's data is one `fdAT` with its own sequence number. Sequence numbers are shared by `fcTL` and `fdAT` and increase by one per chunk.
- `IEND`.

The frame count is not known when `acTL` is written. The writer reserves it, streams every frame, then seeks back and rewrites the count and the chunk checksum before the file is verified. The png crate compresses each frame (09, row 22).

Before the rename the writer reads the temp file back and checks the signature, every chunk checksum, the header size and colour type, the `acTL` count against the number of frame controls, the sequence numbers, the summed delays and the trailer. A mismatch is a `FAIL` and leaves no output.

A dropped stream removes its temp file, as the GIF stream does. The temp name is `<stem>.tmp.<ext>`, for example `out.tmp.png`.

## 12.3 SVG export

Only the tween route exports SVG, because it owns the timeline and the drawing. The export needs no browser, no screenshot and no palette. It applies the same time map, the same prepared plan and the same frame plan as the renderer, so layers, weights, springs, arcs, ballistic motion, rate and loop mode are already in the sampled values.

- The plan is sampled at every presentation time the renderer would use; no extra sample is added at the end.
- One `@keyframes` rule is written per property, named `anikit-<plan index>`, with `linear` timing and percentages taken from the presentation times.
- One element rule selects the target with `[id="..."]`. All animations on one element are listed in a single `animation` declaration, because a second declaration would replace the first.
- The duration is the plan's loop length. `cycle` and `pingpong` use `infinite`; `once` uses `forwards`. Pingpong is not written as `alternate`: the plan already contains the mirrored values when the rate makes them visible, and the repeat is the same restart the GIF does.
- Transform samples become `transform: matrix(a,b,c,d,e,f)` with `transform-box: view-box` and `transform-origin: 0 0`. A transform track replaces an authored `transform` on that element, because the CSS property overrides the presentation attribute.
- Opacity, fill, stroke and stroke width become the matching CSS properties. Fixed decimals are used for numbers and `#rrggbb` for colours, as on the page the renderer drives.
- The last frame's value holds until the end of the iteration, and the restart is the seam. This is what the last frame's delay does in GIF and APNG; a keyframe at 100 percent would instead animate the difference during the hold.
- The style block is inserted immediately before the closing `</svg>` tag. The rest of the drawing is untouched, including its size and `viewBox`; the canvas in the timeline only reports the summary and does not resize the file.
- The drawing must be one SVG document with a closing `</svg>` tag. The style block is not rewritten if the drawing already has one; the exported file is the original plus this block.
- The result is written under a temp name, read back, compared byte for byte, and renamed. A failed export leaves no file.

An SVG export ignores `--loop`, `--colors` and `--dither` and refuses `--preview`. The target element's own CSS animations on the same properties are replaced by the exported rules, as an authored transform is.

## 12.4 Errors

The format adds these reports to part 11:

- the output extension is not one of the three;
- a frame-based route is asked for `.svg`;
- the drawing has no closing `</svg>` tag;
- the timeline has no duration;
- a frame delay is too long for the 16-bit `fcTL` field;
- the written APNG fails its own readback.

Each one prints one `FAIL` line, exits 1 and leaves no output and no temp file.

## 12.5 Tests

- `anikit-core/tests/apng.rs` decodes a written animation with the png crate, checks the control count and play count, and proves that a flipped byte in image data fails the readback.
- `anikit-core/src/stream.rs` covers the APNG merge, delays, an empty stream, a dropped temp file and the refusal of `.svg`.
- `anikit-cli` covers the text route to `.png`, the vector route to `.apng`, and an unknown extension that fails before writing.
- `anikit-cli/tests/tween_fixture.rs` exports the fixture scene to `.svg`, checks the keyframes and the absence of script, feeds the exported file back through the vector route, and asserts that the box moves across the captured frames.

## 12.6 Limits

- APNG has no delta rectangles: every frame is the full canvas. The format allows smaller frames, but the writer has no per-frame disposal state to maintain and the full frame keeps the reader simple.
- SVG export covers the five properties of part 11. Path, raster and grid tracks are not accepted by the route, so they cannot be exported either.
- The exported SVG keeps the drawing's own layout. A drawing whose root size differs from the timeline canvas plays at the drawing's size.
