# Tween sampler and GIF sink: performance and memory

## Scope

Notes for a Rust crate that samples keyframe animation one frame at a time and feeds a GIF encoder. The Python implementation in `agent-skill/bin/gifkit.py` builds a list of every RGB frame and then a second list of every indexed frame before writing (`write_gif`); that is the memory shape to avoid. Covered: frame flow, memory, per-stage cost, existing implementations, work avoidance, parallelism, allocation hygiene and benchmarks.

Measured figures come from a published benchmark or from code that fixes the cost; estimates derive from pixel counts, measured rates or iteration counts in source. Nothing here was built or timed locally.

## Budget

### Frame memory

RGBA8 costs 4 bytes per pixel, an indexed frame 1 byte. Values are MiB.

| Canvas | Pixels | One RGBA8 frame | 100 frames | 300 frames | 1000 frames |
|---|---|---|---|---|---|
| 480x270 | 129,600 | 0.49 | 49.4 | 148.3 | 494.4 |
| 1920x1080 | 2,073,600 | 7.91 | 791.0 | 2373.1 | 7910.2 |
| 3840x2160 | 8,294,400 | 31.64 | 3164.1 | 9492.2 | 31,640.6 |

The Python shape holds RGBA then an indexed copy alongside it: 5 bytes per pixel, 25 percent above the table, plus a 12-frame palette strip in `sample_palette`.

### Where a materialised run swaps or fails

| Limit | 480x270 | 1920x1080 | 3840x2160 |
|---|---|---|---|
| 32-bit process, 2-3 GiB usable | fits to ~6000 frames | fails near 390 frames | fails near 97 frames |
| 2 GiB container or cgroup | fits | 300 frames (2.3 GiB) OOM | 100 frames (3.1 GiB) OOM |
| 16 GiB desktop, ~8 GiB free | fits to ~16,000 frames | 1000 frames (7.7 GiB) swaps; with the indexed copy 9.7 GiB thrashes | 300 frames (9.3 GiB) swaps; 1000 frames (30.9 GiB) is killed |

Below each limit the run succeeds with a large RSS; above it the allocator errors, the process is killed, or the machine swaps. 480x270 is the only size where the naive shape stays small enough to go unnoticed.

### Streaming budget

Three RGBA buffers: the previous composited canvas (needed for delta rectangles and disposal), the current canvas, one render target. The encoder keeps the previous and current indexed frame. Nothing scales with frame count.

| Buffer | 480x270 | 1920x1080 | 3840x2160 |
|---|---|---|---|
| 3 RGBA8 canvases | 1.48 | 23.73 | 94.92 |
| 2 indexed frames | 0.25 | 3.95 | 15.82 |
| LZW output and dictionary | 0.09 | 0.09 | 0.09 |
| Steady working set, 15 percent slack | 2.10 | 32.0 | 127.6 |
| Palette pass peak, frame plus histogram (estimate) | 3.0 | 18 | 72 |
| Expected peak RSS, binary plus allocator (estimate) | 5 | 45 | 170 |

The realistic case is 480x270, 25 fps, 100 frames: peak working set about 2.1 MiB and peak RSS about 5 MiB, against 49 MiB of frames alone in the Python shape. Neither moves at 1000 frames.

### Stage cost per frame

Single core, release build, one palette for the run, no dither.

| Stage | 480x270 | 1920x1080 | 3840x2160 | Scaling | Basis |
|---|---|---|---|---|---|
| Keyframe evaluation, 4 keys per track | 5-30 us | 5-30 us | 5-30 us | tracks | estimate |
| Vector raster, in process | 2-8 ms | 20-80 ms | 80-320 ms | area | estimate; see repo doc |
| Composite, one layer | 0.3-0.6 ms | 5-10 ms | 20-40 ms | area x layers | estimate |
| Resample nearest | 0.08 ms | 1.2 ms | 4.8 ms | destination area | measured, scaled |
| Resample bilinear | 0.9 ms | 15 ms | 60 ms | destination area | measured, scaled |
| Resample Lanczos3 | 3.6 ms | 57 ms | 228 ms | destination area | measured, scaled |
| Colour conversion to float or linear | 0.4-1.0 ms | 6-16 ms | 25-64 ms | area | estimate |
| Palette quantisation | once per run, 0.15-0.5 s | 0.4-1.2 s | 1-4 s | histogram | estimate |
| Remap to palette | 2-6 ms | 30-100 ms | 120-400 ms | area | estimate |
| Dither, Floyd-Steinberg | +1.5-3x remap | +1.5-3x remap | +1.5-3x remap | area | estimate |
| Delta rectangle | 0.15-0.4 ms | 2.5-6 ms | 10-25 ms | area | estimate |
| LZW, full frame | 1.8 ms | 28 ms | 113 ms | delta area | measured rate, computed |
| LZW, 25 percent delta | 0.45 ms | 7 ms | 28 ms | delta area | measured rate, computed |
| Total, no raster or dither, 25 percent delta | 4-9 ms | 50-140 ms | 200-560 ms | | sum |

The measured resize rows come from the `fast_image_resize` README: RGBA8, 4928x3279 to 852x567, single-threaded x86_64 with AVX2, 0.28 ms nearest, 3.53 ms bilinear, 13.26 ms Lanczos3 for a 0.48 MP destination. Rates are 36 Mpx/s for Lanczos3 and 137 Mpx/s for bilinear; columns scale by destination pixels.

The LZW rows use 70 MiB/s of uncompressed input, published with `salzweg`, measured with criterion on a Ryzen 7 2700X and described by its author as rounded. `salzweg` reports itself faster than the `weezl` used by the `gif` crate, so 70 MiB/s is an upper bound.

## Recommended architecture

Two passes. Pass one samples a fixed list of frame indices, adds each to a histogram, builds one global palette and drops the frame. Pass two samples again from index 0 and streams each frame to the encoder. Re-sampling is cheap only if `render(i)` is pure in `i`, so purity is part of the contract.

The sink owns the canvases and encoder state, the sampler owns the render target. Three RGBA buffers suffice because no frame older than the previous one is needed. A frame is borrowed into `push` and returned to the pool, so nothing is cloned and the pool size is fixed.

```rust
pub struct Canvas { w: u16, h: u16, px: Vec<u8> }          // RGBA8, w*h*4
pub struct FrameRef<'a> { w: u16, h: u16, px: &'a [u8], stride: usize }

pub struct Sampler {
    timeline: Timeline,
    tables: SampleTables,        // solved easing per segment, resolved keys
    state: Vec<Resolved>,        // one slot per track, reused every frame
    canvas: Canvas,              // render target, reused
    dirty: Rect,                 // union of track bounds touched this frame
}

impl Sampler {
    pub fn new(timeline: Timeline) -> Result<Self, Error>;
    pub fn frame_count(&self) -> u32;
    /// Pure in `index`: same index, same pixels, independent of call order.
    pub fn render(&mut self, index: u32) -> FrameRef<'_>;
    /// Resolves tracks and hashes them; no pixels are touched.
    pub fn state_hash(&mut self, index: u32) -> u64;
}

pub struct Encoder<W: Write> { /* palette, prev canvas, prev indexed, pool */ }

impl<W: Write> Encoder<W> {
    pub fn new(out: W, canvas: (u16, u16), palette: &Palette) -> Result<Self, Error>;
    /// Borrows one frame, writes it, returns when it is in the BufWriter.
    pub fn push(&mut self, index: u32, pts_cs: u32, frame: FrameRef<'_>) -> Result<(), Error>;
    /// Flushes with the end timestamp, so the last frame gets its full delay.
    pub fn finish(self, end_pts_cs: u32) -> Result<Stats, Error>;
}

pub fn run(sampler: &mut Sampler, out: impl Write) -> Result<Stats, Error> {
    let palette = build_palette(sampler, &PALETTE_PICKS)?;   // pass 1, 8 to 12 frames
    let sink = BufWriter::with_capacity(128 * 1024, out);
    let mut enc = Encoder::new(sink, sampler.canvas_size(), &palette)?;
    let mut last_hash = None;
    for i in 0..sampler.frame_count() {
        let hash = sampler.state_hash(i);
        if Some(hash) == last_hash { continue; }                // held frame, delay is absorbed
        last_hash = Some(hash);
        let f = sampler.render(i);
        enc.push(i, pts_cs(i), f)?;
    }
    enc.finish(total_cs())
```

Delays are computed inside the sink from `pts_cs`, as `gifski` does in `Writer::write_frames`, so a skipped frame extends the previous one. Clamp the floor at 2 centiseconds and cap long holds at 100.

For a pipeline, use two bounded channels and `max(2, cores/2)` frames in flight: `sampler -> bounded(2) -> encoder -> BufWriter`. Frames are reordered by index at the writer, as `gifski` does with `ordered_channel::bounded(0)`. In-flight count is canvases, not frames.

Borrowed frames are the default: a slice and a stride keep the buffer alive only for `push`. `Arc<Canvas>` is only for a stage that outlives the call.

gifski's temporal handling is not a dither kernel run over time. `denoise.rs` keeps a 5-frame lookahead and picks each pixel from a 9-neighbour median across frames; `set_importance_map` weights pixels by how long they stay visible; and remap passes the previous frame through `Image::set_background`, so a pixel matching the background is written as transparent instead of being redithered. pngquant uses the same library for still images; the animation-specific parts are gifski's.

## Optimisation list

Ordered by expected gain for the realistic 480x270, 100-frame job. Items 2, 4 and 5 rest on measured timings.

1. One palette per run. Removes per-frame quantisation. `gifkit.py` already does this with 12 picks, and `gifski` and `ffmpeg` do the same. imagequant's default speed 4 is 12 k-means iterations plus up to 20 feedback trials (`Attributes::set_speed`), all removed from the per-frame path. A median cut splits the box with the highest weight times variance at the weighted median, then k-means refines it. Basis: iteration counts in source.
2. Delta rectangles and skipped unchanged pixels. Encode only the changed box. `gifski`'s `trim_image` crops against the post-disposal screen and `ffmpeg`'s `paletteuse diff_mode=rectangle` computes a bounding rectangle of changed pixels. LZW and remap scale with area, so a 10 percent box is roughly 10x on both. Basis: measured LZW rate applied to area.
3. Held and duplicate frames. Test the resolved scene state before rendering, not the pixels after. `gifski` compares whole frames (`next_image.as_ref() == image.as_ref()`) and skips; a 64-bit hash of resolved track values also skips the render. Basis: exact, gain is 1/(duplicate fraction).
4. Fill the canvas at final size and skip resampling. `gifski` resizes with Lanczos3 when the input needs it, because it accepts arbitrary PNG input; a tween knows its output size. Removing the stage saves the whole row. Basis: measured resize table.
5. Bilinear or nearest where the art is flat. Nearest is 12.6x cheaper than bilinear and 47x cheaper than Lanczos3 in the measured table; for solid fills and hard edges, nearest at integer scale is exact. Basis: measured ratios.
6. Static layer cache. A track whose keys are equal across a span contributes the same pixels; composite it once into a background canvas and skip it after. Basis: estimate, proportional to layer area.
7. Parallel remap. imagequant parallelises k-means over `par_chunks_mut(256)` and remapping over rows with `par_bridge`; `gifski` caps quantise threads at `min(max_threads, 4)`. Worth it at 1080p and above, marginal at 480x270. Basis: Amdahl, 70-80 percent parallel.
8. Buffer pool and zero allocation per frame. Not a throughput win at 480x270, but it keeps RSS flat. Target is zero allocations per frame after warmup.
9. Solved easing tables. Solve each bezier segment once into 256 steps and index it; per-track cost falls to a lookup. Negligible against pixels, relevant at 1000 frames with many tracks.

Sampling only changed pixels is only meaningful per track, because a compositor still walks the canvas to clear and re-blend. The better lever is a union of per-track dirty rectangles.

## Parallelism

Per-frame parallelism is the wrong default: frame i+1's delta depends on what frame i left on screen, so compositing and delta computation are sequential. What parallelises is work inside a frame (remap rows, resampling) and the overlap between rendering and encoding.

`gifski` is the worked example, and it does not use rayon for its pipeline. It spawns scoped `std::thread` pools per stage (`minipool::new_scope`), connects them with `crossbeam_channel::bounded(0..2)`, reorders with `ordered_channel`, and caps each pool: resize at `min(max_threads, 4)` or 6 in fast mode, quantise at `min(max_threads, 4)`, LZW at 1 or 3 in fast mode. `max_threads` is `thread::available_parallelism()`. Rayon appears only inside its dependencies (`resize`, `imagequant`).

Worker counts: 1 below 8 frames or at 480x270, `min(cores, 4)` for remap at 1080p and above, 1 for the writer. LZW is sequential inside a stream.

Thread pool startup is tens of microseconds per native thread on Linux, more on Windows because of stack commit and TLS. A global rayon pool lives until exit, waste for a 40 ms run, so build one per run only when the frame count justifies it.

Parallelism is a net loss under about 50 ms of work, below 8 frames, at 480x270 or smaller, or when a serial stage exceeds 20 percent. It is also a determinism risk: imagequant merges floating-point k-means sums per chunk, so summation order follows chunking. Identical output across thread counts must be verified.

## Benchmark plan

### Cold start budget

| Segment | Target | How measured |
|---|---|---|
| Process start to `main`, Linux warm | <= 15 ms median | `hyperfine --shell=none --warmup 5` on `--version`, 30 runs |
| Process start to `main`, Windows | <= 30 ms median | same, plus one cold run after a reboot |
| `main` to palette ready, 480x270, 8 picks | <= 400 ms | internal `Instant` to stderr, subtracted from wall time |
| Full run, 480x270, 100 frames | <= 1.7 s total | `hyperfine`, exec to exit |

No browser on the hot path. A Chromium start costs hundreds of milliseconds and would dominate every other number here; raster belongs to the caller.

### Per-stage targets

| Metric | Fixture | Target | Method |
|---|---|---|---|
| Frame, 480x270 | moving rect, text, static background | <= 6 ms median | criterion, 100 iterations |
| Frame, 1920x1080 | same scene | <= 100 ms | criterion |
| Frame, 3840x2160 | same scene | <= 400 ms | criterion |
| Remap, 480x270, 256 colours, no dither | RGBA8 | <= 4 ms | criterion |
| LZW, 480x270 indexed | 25 percent delta | <= 1 ms | criterion, encoder alone |
| LZW share of frame | 480x270 | <= 25 percent | criterion, encoder vs whole frame |
| Palette build | 480x270, 8 picks, speed 8 | <= 400 ms | criterion |
| Frame scaling | 100 vs 1000 frames | <= 5 percent drift | criterion, parameterised count |
| Peak heap | 480x270, 1000 frames | <= 3 MiB | dhat peak |
| Peak heap | 1920x1080, 300 frames | <= 45 MiB | dhat peak |
| Heap vs frame count | 100 vs 1000 frames | <= 64 KiB difference | dhat peak, two runs |
| Allocations per frame | steady state | 0 | dhat allocation count |
| Instructions per frame | 480x270, no raster | <= 20 M | gungraun (iai-callgrind) |
| Output determinism | 1 vs 8 threads | byte-identical | hash over 100 runs |

### Tooling

criterion measures wall-clock time statistically and suits the targets above, but is noisy in CI. gungraun, the current name of iai-callgrind, runs a benchmark once under Valgrind Callgrind and reports instruction counts and cache behaviour; it is precise across machines, Linux only, and about 10-50x slower. Use criterion for targets and gungraun for inner-loop regressions.

Allocation counting belongs in the test suite. dhat reports peak bytes and allocation counts, and `dhat::assert` fails a test when a frame allocates or peak heap exceeds a bound. Run 100 and 1000 frames and assert peak heap differs by less than 64 KiB; that catches the whole class of frame-retention bugs.

Peak RSS is separate from peak heap: allocator retention and stack reservations are not in the heap total. Read `VmHWM` from `/proc/self/status` on Linux and `PeakWorkingSetSize` from `GetProcessMemoryInfo` on Windows. For whole-process timing use `hyperfine --shell=none`.

Fixtures: 480x270 with a moving rectangle, a fading logo and a static background; the same at 1080p and 4K; an ASCII grid; 30 percent held frames; a full-frame crossfade.

## Sources

- `ImageOptim/gifski`, `src/lib.rs`: `Writer::write_inner` (stage threads, channels), `quantize_frames` (identical-frame skip, disposal), `remap_frames` (`gif_dispose::Screen`, `trim_image`), `write_frames`. `src/minipool.rs`: `new_channel`, `new_scope`. `src/denoise.rs`: `LOOKAHEAD = 5`, `Acc`, `smart_blur`. `src/encoderust.rs`: `compress_frame`.
- `ImageOptim/libimagequant`, `src/attr.rs`: `Attributes::set_speed` (iterations, trials, histogram cap). `src/quant.rs`: `find_best_palette`. `src/mediancut.rs`: `take_best_splittable_box`, `split`, `prepare_sort`. `src/kmeans.rs`: `Kmeans::iteration`, `par_chunks_mut(256)`. `src/nearest.rs`: `Nearest::new`, `vp_create_node`, `LEAF_MAX_SIZE = 6`. `src/remap.rs`: `remap_to_palette`.
- `FFmpeg/FFmpeg`, `libavfilter/vf_palettegen.c`: `compute_box_stats`, `get_next_box_id_to_split`, `split_box`, `update_histogram_diff`. `libavfilter/vf_paletteuse.c`: `set_processing_window` (`diff_mode=rectangle`), dither modes, default `DITHERING_SIERRA2_4A`.
- `ImageMagick/ImageMagick`, `MagickCore/layer.c`: `CoalesceImages` (one cloned canvas per frame), `DisposeImages`, `OptimizeImageLayers`, `CompareImagesBounds`.
- `image-rs/image-gif`, `src/encoder.rs`: `write_frame`, `write_image_block` (one reused buffer), `write_lzw_pre_encoded_frame`. `src/common.rs`: `Frame::from_rgba_speed` via `color_quant::NeuQuant`.
- `kornelski/image-gif-dispose`, `src/screen.rs`: `Screen::new`, `dispose_only`, `then_blit`. `src/disposal.rs`: `SavedState::Previous(Vec<RGBA8>)`.
- `Cykooz/fast_image_resize`, `README.md`: RGBA8 4928x3279 to 852x567, nearest 0.28 ms, bilinear 3.53 ms, Lanczos3 13.26 ms, single-threaded x86_64 AVX2.
- `redwarp/lzw` (`salzweg`), `README.md`: LZW encode 70 MiB/s variable, 120 MiB/s fixed 12-bit, criterion on a Ryzen 7 2700X.
- `iai-callgrind/iai-callgrind` (Gungraun), `README.md`: one-shot Valgrind measurement, instruction counts, Linux only.
- `nnethercote/dhat-rs`, `README.md`: heap profiling, allocation assertions.
- `sharkdp/hyperfine`, `README.md`: `--shell=none`, warmup for sub-5 ms commands.
- Repository: `agent-skill/bin/gifkit.py` (`write_gif`, `sample_palette`, `merge_runs`), `docs/research/keyframe-tween-animation-info.md` (Chromium capture cost, palette churn), `docs/research/python-to-rust-refactor-pitfalls.md` (per-iteration allocation, `BufWriter`, `with_capacity`).
