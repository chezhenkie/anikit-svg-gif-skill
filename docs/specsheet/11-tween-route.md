# 11-tween-route

Status: implemented. The fourth route renders a static SVG drawing through a JSON timeline that the tween crate compiles, and exports the same prepared plan as an animated SVG. The model is parts 04 to 07; this part fixes the file grammar, what the route draws and how it fails. The output formats are part 12.

## 11.1 What the route adds

The three earlier routes animate input that already moves, or a sequence that already exists. This route animates a drawing that does not move at all, from a timeline the author writes: position, opacity, fill, stroke and stroke width over time, with layers, easing, springs, arcs and ballistic motion. It is the first consumer of the tween crate.

The route opens one drawing in one browser page, writes the sampled state of every target element, and captures the canvas once per frame. It needs the same browser as the vector route; part 02 and 09, row 21 record it.

The output extension chooses the format. `.gif` and `.png`/`.apng` take the browser path; `.svg` samples the prepared plan and writes CSS keyframes without starting a browser, as part 12 fixes. A frame output follows part 03, and an SVG export is not a frame stream.

Path, raster and grid tracks are not accepted. Their layer composition is still open (parts 06 and 07), and the route does not need them.

## 11.2 Input

One JSON file in the canonical shape of 04.6. Times are absolute milliseconds. Keys are in time order and easing belongs to the key the segment leaves.

```json
{
  "version": 1,
  "canvas": {"w": 120, "h": 80, "bg": "#ffffff", "fps": 10},
  "timing": {"loop": "cycle", "rate": 1.0},
  "tracks": [
    {"id": "box-move", "kind": "svg.transform", "src": "art.svg#box",
     "keys": [
       {"t": 0, "translate": [0, 0]},
       {"t": 1000, "translate": [60, 0]}
     ]},
    {"id": "box-fade", "kind": "svg.opacity", "src": "art.svg#box",
     "keys": [
       {"t": 0, "value": 1.0},
       {"t": 1000, "value": 0.4}
     ]}
  ],
  "layers": []
}
```

Root fields: `version` (1), `canvas`, `timing`, `tracks`, `layers`. Only `timing` and `layers` may be absent. Unknown fields fail at every level, so a misspelt option is reported instead of ignored.

### 11.2.1 Canvas and timing

| field | type | default | rule |
|---|---|---|---|
| `w`, `h` | integer | required | 8 to 8192 each |
| `bg` | colour | `#000000` | page background and the matte |
| `fps` | integer | required | 1 to 50, the frame grid |
| `loop` | string | `once` | `once`, `cycle` or `pingpong`, the time map of 04.5 |
| `rate` | number | `1.0` | finite; negative reverses time, zero freezes it |

`loop` is the time map, not the GIF repeat count. The repeat count is `--loop`. A negative `rate` mirrors time; under `once` it clamps to the start, so reverse playback belongs with `cycle` or `pingpong`.

### 11.2.2 Colour

`#rgb` or `#rrggbb`, case-insensitive, opaque. Alpha is not in the grammar: partial colour bindings are unimplemented (07) and GIF has no partial alpha.

### 11.2.3 Tracks

Every track needs a unique `id`, a `kind`, a `src` and a non-empty `keys` array. `smooth` is optional and applies to the whole track.

| kind | value | slot | drawn as |
|---|---|---|---|
| `svg.transform` | translate, rotate, scale, skew | 0 | matrix on a wrapper `g` around the element |
| `svg.opacity` | number 0 to 1 | 1 | inline `opacity` on the element |
| `svg.fill` | colour | 2 | inline `fill` on the element |
| `svg.stroke` | colour | 3 | inline `stroke` on the element |
| `svg.stroke-width` | number 0 or more | 4 | inline `stroke-width` on the element |

`src` is `file.svg#elementId`, resolved relative to the timeline file. Every track must name the same drawing; the first track fixes it and a second file is an error naming both. The fragment must be an element inside the drawing, not the root `svg`.

Two tracks may target one element when their kinds differ. Two tracks of one kind on one element are an error.

A key is an object with `t` in milliseconds (non-negative integer), optional `easing`, optional `motion`, optional `tangent`, and the value fields of its kind.

- `svg.transform`: `translate` as `[x, y]` in pixels, `rotate` in degrees, `scale` as a number or `[x, y]`, `skew` in degrees. A component missing from a key keeps the value of the previous key; the first key starts from `translate [0, 0]`, `rotate 0`, `scale 1`, `skew 0`. A key may carry none of them and inherit all.
- `svg.opacity`: `value`, 0 to 1.
- `svg.fill`, `svg.stroke`: `value`, a colour.
- `svg.stroke-width`: `value`, 0 or more.

`smooth` is `eased` (default), `auto` or `monotone`, as in 05. `auto` and `monotone` raise the curves through linear segments; an explicit easing still applies.

### 11.2.4 Easing

`easing` is a string or an object:

| value | meaning |
|---|---|
| `linear` | no change of rate |
| `ease` | cubic bezier (0.25, 0.1, 0.25, 1) |
| `ease-in` | cubic bezier (0.42, 0, 1, 1) |
| `ease-out` | cubic bezier (0, 0, 0.58, 1) |
| `ease-in-out` | cubic bezier (0.42, 0, 0.58, 1) |
| `hold` | the value stays until the next key, then jumps |
| `{"bezier": [x1, y1, x2, y2]}` | explicit cubic, x handles in 0 to 1 |
| `{"steps": n, "at": "end"}` | stepped; `at` is `start`, `end`, `none` or `both` |
| `{"spring": {...}}` | closed-form spring, 05.5 |

Spring fields: `stiffness` 400, `damping` 30, `mass` 1, `v0` 0, `settle` 400 ms, `restDelta` 0.5, `restSpeed` 2. `settle` is the end of the segment, so a spring that is still moving there fails at prepare, as 10.2 requires.

The last key's easing is ignored, because it starts no segment.

### 11.2.5 Motion

`motion` replaces `easing` on one key and is accepted only on scalar tracks (`svg.opacity`, `svg.stroke-width`), because the crate's ballistic and arc segments produce an absolute scalar (10.2). Using both on one key is an error.

```json
{"t": 0, "value": 100, "motion": {"ballistic": {"plane": 0, "restitution": 0.5}}}
{"t": 0, "value": 0, "motion": {"arc": {"bend": 0.1, "easing": "ease-in-out"}}}
```

Ballistic fields: `plane` required; `gravity` 1400 px/s^2, `v0` 0 px/s, `restitution` 0.5, `friction` 0.3, `minBounce` 1.0 px, `maxSpeed` 1000 px/s, `contactHold` 40 ms, `squash` 0.2, `stretch` 0.15. The segment ignores the next key's value while it runs and ends at `plane`; prepare refuses a next key that differs from the plane by more than 0.5 px.

Arc fields: `bend` required, a fraction of the chord; `easing` optional, default `linear`.

`tangent` is an explicit outgoing tangent in value units per second, scalar tracks only, used by `auto` and `monotone` smoothing.

### 11.2.6 Layers

A layer applies one or more tracks over the base track they name. It is the shape of 07.3 in JSON:

```json
"layers": [
  {"id": "flash", "order": 1, "window": [300, 700],
   "weight": [{"t": 0, "value": 1}, {"t": 700, "value": 0}],
   "bindings": [
     {"target": "box-fade", "op": "add",
      "keys": [{"t": 0, "value": 0.2}]}
   ]}
]
```

- `id` unique, `order` an integer 0 or more and unique. Order decides application, never array position.
- `window` is optional `[start, end]` in milliseconds, start before end. It does not extend the duration.
- `weight` is optional scalar keys, default a constant 1. It is in 0 to 1 unless the binding sets `overshoot`. A weight key carries `t`, `value`, optional `easing` and `tangent`; `motion` is not accepted there.
- `bindings` is non-empty. `target` is a base track id. `op` is `replace` (default), `add`, `multiply` or `accumulate`; `mode` is `append` (default) or `prepend` and applies to transform targets only; `overshoot` defaults to false; `allowColorAdd` defaults to false, because colour is not additive (07.4). `weight` is an optional per-binding scalar track. `keys` uses the same value grammar as the target track.
- A `mask` is not in the grammar. Spatial masks are unimplemented (07.5).

### 11.2.7 Duration

The duration is the largest key time over base tracks, binding tracks and weights. Windows do not extend it. A scene whose keys all sit at zero is a still: the plan asks for two frames, the writer merges them into one.

## 11.3 Frame flow

The route follows part 03 like the other three.

- `count = max(2, ceil(duration_s * fps))`, doubled when `loop` is `pingpong`, then `FramePlan::uniform(count, fps)` gives the 10 ms grid and the presentation times. The last frame starts on the last grid point before the duration and holds to it. The vector route rounds the count.
- The palette pass captures up to 12 frames spread across the plan for GIF only; APNG renders each frame once. The stream then starts.
- Each frame samples every property into one reused buffer at the frame's presentation time, which the plan maps through loop mode and rate. The result is written into the page, captured as a PNG, matted onto `bg` and pushed at that same presentation time.
- `--colors`, `--dither` and `--loop` behave as on the other routes for GIF; `--colors` and `--dither` have no effect on APNG, and `--loop` sets the APNG play count. `--preview` writes the first captured frame and is refused for an SVG export (part 12).
- Nothing is sampled or written before the scene has parsed and prepared, so a bad timeline leaves no file and no temp file.

## 11.4 The page

One SVG is wrapped in one page at the canvas size, exactly as the vector route wraps it: the drawing fills the page viewport, and with a `viewBox` its content is fitted and centred by `preserveAspectRatio`, with `bg` filling the rest. The page loads once. Fonts are awaited, every animation clock is paused at its start, and the drawing's own CSS, SMIL or WAAPI animation is never captured; the timeline is the only motion.

The vector route, which shares this wrapper, takes its canvas from `--width` and `--height`, then from the drawing's root size: a unitless number or `px` is pixels, `pt`, `pc`, `mm`, `cm` and `in` convert at 96 px per inch, and a percentage resolves against the viewBox side, then the other explicit side, then the 800x600 fallback. A missing side follows the other side's ratio.

For every target element the page carries a rule that sets `animation` and `transition` to `none` with priority before it loads, so the element's own clocks never start. The setup script then inserts an empty `g` immediately before the element and moves the element into that `g`; it also cancels any remaining animation object and removes the element's SMIL animation children, which covers a script driven animation. A scene imported from that drawing therefore cannot apply the source motion a second time, and no compositor layer from a started clock survives. Static attributes stay, so the sampled matrix still composes with the element's authored transform. The wrapper has no other attributes, so the picture does not change before the first frame. Per frame:

- `svg.transform` writes `transform="matrix(a,b,c,d,e,f)"` on the wrapper, in user units, about the wrapper's origin. The matrix sits on the parent side, so it composes with authored transforms instead of replacing them, and the track moves the element in its parent's axes. This is why no `transform-origin` rule is involved: the transform attribute has no ambiguity under a shifted view box.
- `svg.opacity`, `svg.fill`, `svg.stroke` and `svg.stroke-width` write inline styles on the element itself.
- Numbers are formatted with fixed decimals and colours as `#rrggbb`, so the page receives the same text on every platform.

The element is looked up by id after the page loads. A missing id, an id that names the root, or a non-SVG element fails the run before the palette pass, with the id in the message.

## 11.5 Visible behaviour and errors

The summary line is the one the other routes print: `<name>  N frames, X.XX s, W x H`. An SVG export reports `N samples` instead, because it writes keyframes rather than captured frames; part 12 has the details.

A failed run prints one `FAIL` line, exits 1 and writes nothing. The route reports, naming the place in the file where it can:

- the file is missing or not JSON;
- `version` is not 1, a required field is missing, an unknown field is present, or a value has the wrong type or range;
- a track or layer id repeats, an order repeats, a key time goes backwards, or two keys share a time without the earlier one holding;
- two tracks take one kind and one element, or a binding targets a missing track;
- a `src` file differs from the first one, has no fragment, is not `.svg`, or cannot be read;
- the drawing has no element with that id, or the id names the root;
- a mask field, or a path, raster or grid kind, is asked for.

The browser and its temporary profile are removed on success and on failure. The drawing's own animations are paused, and a target's own animation is switched off rather than paused, so no frame carries it.

## 11.6 Tests

- Unit tests in `anikit-render-svg` cover the grammar: the example above parses, transform components inherit, duration and frame count follow the last key, layers resolve to the right target, colours and easings map to the crate, and each rejected shape reports the place.
- `anikit-cli/tests/tween_fixture.rs` runs a scene with three base tracks and a windowed add layer over a 120x80 drawing, compares frame count, delays, size and per-frame pixel hashes, checks that the box moved and faded, and runs the scene twice for byte equality. A second scene checks that `svg.stroke` and `svg.stroke-width` reach the page: a border changes from thin black to thick red.
- The fixture has no frozen Python reference; the first Rust run is recorded in its manifest, because this route did not exist before the port.
- A scene that names a missing element fails with the id in stderr, exits 1 and leaves no output. The test is skipped when no browser is installed, like the vector fixture.
- An SVG export of the fixture needs no browser, contains its keyframes and no script, and plays through the vector route with the box moving across the captured frames.

## 11.7 Limits

- One drawing per scene. Several files would need several viewports or a shared coordinate space, and no user has asked for one.
- The root `svg` cannot be a target, because the route wraps targets in a `g`.
- The drawing should be static. Its own animation is paused; a JavaScript canvas or WebGL drawing is out of scope, as on the vector route.
- No path, raster or grid kinds, no spatial masks and no alpha-only colour. Those are the open rows named by parts 06, 07 and 09; a route that needs one closes it with a test in its own part first.
- The browser is required for GIF and APNG, so those outputs are unsupported where the vector route is unsupported (02.5, 09 row 20). The SVG export needs no browser.
- SVG export covers the five property kinds and replaces any authored transform or CSS animation on those properties of a target element. Part 12.
