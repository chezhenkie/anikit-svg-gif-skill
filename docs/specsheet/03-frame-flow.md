# 03-frame-flow

Status: the streaming sink is implemented and used by the images and vector routes. The delta rectangles of section 3.3 are not written yet, so frames go out whole with disposal 1, and the text route still uses `write_gif` with explicit delays.

## 3.1 Why two passes

A GIF needs one palette for the whole loop, and the palette must exist before the first frame is encoded. That forces two passes over the animation: a cheap sampling pass that builds the palette, then a rendering pass that quantises and writes.

The sampling pass takes up to 12 frames spread evenly across the loop, which is the rule the frozen Python already follows (section 1.2). It must be cheap enough to run before anything is written, so `tween` sampling is a pure function of time with no allocation, and a renderer's frame build for this pass may render at reduced size or reduced quality when the route allows it.

## 3.2 The sink

The writer owns the palette, the previous canvas and the disposal state. The renderer pushes frames into it.

```rust
pub trait FrameSink {
    fn push(&mut self, index: u32, pts_us: u64, frame: &Frame) -> Result<(), Error>;
    fn finish(self) -> Result<GifResult, Error>;
}
```

- `push` takes a presentation time, not a duration. The writer derives the delay from the difference between consecutive `pts_us`, so cumulative rounding stays in one place and a dropped frame cannot corrupt the grid.
- `index` is the frame's position in the plan. It lets the writer skip a frame that repeats the previous one and add its delay to the run, which is the merge rule.
- Held frames are detected in the plan and never rendered. A hold of one second at 12 fps is one frame with a 1000 ms delay, not twelve identical frames.

## 3.3 Disposal and deltas

Every GIF frame after the first is a delta against the canvas left by the previous one, with disposal method 1 (leave in place). The writer therefore needs the previous canvas after disposal, and frames cannot be evaluated fully independently: the sink is ordered and synchronous.

A two-slot pipeline is the limit of what is safe. The renderer borrows a buffer, the writer encodes it, and the buffer returns to the pool. More slots buy nothing, because the writer cannot proceed past a frame it has not received.

## 3.4 Memory budget

| canvas | RGBA8 per frame | 100 frames held | 1000 frames held |
|---|---|---|---|
| 480x270 | 518 KB | 52 MB | 518 MB |
| 1920x1080 | 8.3 MB | 830 MB | 8.3 GB |
| 3840x2160 | 33 MB | 3.3 GB | 33 GB |

Holding the loop is what the Python implementation does and what the port must not do. With a two-slot pipeline the working set is two frames plus the palette and the encoder state. Measured for the realistic case, 480x270 with 100 frames: peak working set 2.1 MiB and peak process memory about 5 MB, flat when the frame count rises to 1000.

## 3.5 Determinism

- No wall clock anywhere in the pipeline. Time comes from the plan, not from elapsed time.
- Integer microseconds in, one conversion to `f64` per sample.
- Parallel work is chunked by rows or by frame with a fixed order of output, never by "first finished".
- A random or threaded dither would break byte equality; dithering is a fixed error-diffusion pass.

## 3.6 Tests

- A streaming run and a materialised run of the same plan produce byte-identical GIF files.
- The palette pass and the render pass agree on frame count and loop duration.
- A 1000-frame run keeps peak memory within a stated multiple of a 100-frame run.
- A hold produces one frame with the summed delay, not repeated frames.
- A failed write leaves no output file and no temp file.
