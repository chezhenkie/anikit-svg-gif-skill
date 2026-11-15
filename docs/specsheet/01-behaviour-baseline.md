# 01-behaviour-baseline

Status: the frozen Python at tag python-freeze, main 08fd195, deleted from main at 0.2.0. This part records what the Rust binary reproduces. A rule marked as a defect is not a decision and the port may correct it, but the correction is stated in 09 first.

## 1.1 Routes and flags

Three routes, one shared core, one output type.

| route | input | entry point |
|---|---|---|
| text | a text file with frames, or a folder of .txt files | ascii_gif.py / anikit ascii |
| images | a folder of images, sorted by name | pics_gif.py / anikit images |
| vector | an .svg or .html file, captured in a browser | svg_gif.py / anikit vector |

Text route flags: `--font`, `--size` (default 28), `--fg` (default `#e6edf6`), `--bg` (default `#0a0d12`), `--delay` (default 100 ms), `--width` and `--height` for a fixed canvas with the grid centred. The port draws with an embedded face, so `--font` is an override rather than the only way to get stable metrics (09, row 11).

Text input format: frames separated by a line containing only `---`, optionally with a delay in milliseconds (`--- 250`). A directory contributes one frame per .txt file, sorted by name. ANSI SGR sequences for 16 colours, 256 colours and truecolor set the glyph colour per cell; other CSI sequences are stripped. Tabs expand to four spaces.

Images route flags: `--mode sequence|crossfade|kenburns` (default crossfade), `--fps` (default 12, maximum 50), `--hold` (default 1.2 s), `--transition` (default 0.5 s), `--zoom` (default 1.10), `--fit cover|contain` (default cover), `--bg` for contain padding, `--width` (default 800), `--height` (default from the first image).

Images input: png, jpg, jpeg, webp, bmp, tif, tiff and gif, sorted naturally, EXIF rotation applied. Sequence holds each image for `--hold`; crossfade holds then blends into the next; kenburns holds a slow zoom that alternates direction between images, with the same optional crossfade.

Vector route flags: `--width`, `--height` (default from the drawing), `--duration` (default 2 s), `--fps` (default 15), `--animate none|rotate|pulse|fade` (default none), `--selector`, `--background`, `--dpr` 1 or 2, `--browser PATH`, and the environment variable `GIFKIT_BROWSER`.

Vector input: an .svg is wrapped in a page at the requested viewport; an .html page is opened as is. CSS animations, WAAPI animations and SMIL are paused and seeked to explicit times, so frames do not depend on wall clock. A JavaScript canvas page needs its own frame hook and is not driven by the tool.

Flags on all routes: `-o` or `--out`, `--colors` 2 to 255 (default 255), `--dither none|floyd` (default none), `--loop` (0 means forever), `--preview` for a PNG of the first frame.

## 1.2 GIF core rules

- Palette: up to 12 frames spread evenly across the loop, all frames when there are 12 or fewer, stacked into one image and quantised by median cut to `--colors`. Every frame is mapped onto that one palette.
- Delays: each delay is `round(cumulative_ms / 10) * 10`, at least 20 ms, and the emitted value is the difference from the previous total, so rounding error cannot accumulate. Frame rate is capped at 50.
- Merge: identical consecutive frames collapse into one frame whose delay is the sum.
- Write: disposal method 1, the requested loop count, optimisation off, temp file `<stem>.tmp.gif` beside the target. On any failure the temp file is removed.
- Verify: the file is reopened, frames counted, durations summed and dimensions compared. A duration difference above 10 ms fails. Only after that is the temp file renamed over the target.

## 1.3 Error contract

- One line on stderr beginning with `FAIL`, exit code 1, no output file, no temp file left behind.
- Every route raises the same error type, so the message shape is uniform across routes.
- Covered cases: missing input, empty folder, no .txt frames, invalid colour, colours outside 2 to 255, frame and duration count mismatch, frame size mismatch, unknown mode or dither, frame rate of zero or less, no usable browser, and a written file that fails verification.

## 1.4 Port obligations

Must match: flag names, defaults and validation; the text frame format and SGR handling; image ordering and EXIF rotation; the three image modes; the vector presets; palette sampling; delay arithmetic; merge; atomic write and verification; the error shape; the preview frame.

May change: the implementation language, memory use (streaming replaces the frame list), font discovery internals, browser discovery internals, and the wording of messages as long as the contract holds.

Known defect: an animated GIF or animated WebP source contributes only its first frame, with no message. The port's behaviour is decided in 09, row 9.

Known defect: an explicit `--font` path that does not exist is ignored and the next candidate is used. The port fails instead, because a silently different face changes the grid and the frame size.
