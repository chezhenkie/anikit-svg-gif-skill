# 07-tween-composition

Status: implemented for number, opacity, transform and colour. Path, raster and grid composition, partial colour bindings and spatial masks remain unimplemented. Evidence: docs/research/tween-composition.md. This part is the reason a shake or a camera move can be reused without rewriting the animation it sits on.

## 7.1 Purpose

A layer is an animation applied on top of another. The intended use is reuse: a shake, a pulse, a drift, a camera move or a colour grade is written once and applied to many animations. A weight over time lets an effect fade in and out, so it can start or stop without a cut.

The stack is per property. That is the unit Web Animations composites, and it is the only unit where "add" has a defined meaning.

## 7.2 The three rules

1. Weight scales the effect, not the value: `v = mix(v, combine(v, x), w)`. With `w = 0` the result is exactly the value beneath, with `w = 1` it is exactly the combined value, for every operation including multiply and transform concatenation. Scaling the value instead would not scale the effect and would break both endpoints.
2. Order is an explicit integer, sorted on, with duplicates rejected at prepare. Neither array position nor element id may decide the output, because a rewritten array or a renamed element would then change the animation.
3. Clamp once, after the whole stack, and reject an add that the type cannot express instead of silently replacing.

At `w = 1` the mix is skipped, so the exact path stays exact. The weight is itself a track, so a fade is two keys.

## 7.3 Model

```rust
pub struct LayerId(u32);
pub struct PropId { pub element: u32, pub slot: u16 }

pub struct Layer {
    pub id: LayerId,
    pub order: u32,
    pub window: Option<Window>,   // hard time mask, microseconds
    pub weight: Track,            // scalar, the layer default
    pub mask: Option<Mask>,       // per-pixel types only
    pub bindings: Vec<Binding>,
}

pub struct Binding {
    pub target: PropId,
    pub op: Op,
    pub track: Track,
    pub weight: Option<Track>,    // overrides the layer weight
    pub mode: AddMode,            // transform only
    pub overshoot: bool,          // permits weight outside 0 to 1
}

pub enum Op { Replace, Add, Multiply, Accumulate }
pub enum AddMode { Append, Prepend }
```

Prepare resolves the stack into either a single prepared track or one contiguous range of prepared layers. A property with zero or one contributing `Replace` binding takes the single path, which is the current code path with no per-frame cost. Everything else takes the stacked path, where sampling walks the range, skips closed windows and zero weights, and pays for the mix only when the weight is strictly between 0 and 1.

Prepare folds the stack below a full-weight, unwindowed `Replace` binding: that binding becomes the base and the layers under it are not sampled. A constant `Add` or `Multiply` binding stays a layer and is sampled each frame. A binding to a missing element fails at prepare, never at render.

## 7.4 Per-type rules

| type | replace | add | multiply | note |
|---|---|---|---|---|
| number | default | sum | product | clamp only where the property declares a range |
| opacity | default | sum, then clamp to 0 to 1 | product | add saturates, independent overlays need multiply |
| transform | default | list concatenation | matrix product | append keeps the layer in the base axes, prepend in screen space; add with scale or perspective is rejected |
| colour | default | RGB sum, premultiplied | premultiplied product | CSS says colour is not additive, so add needs explicit permission |
| path | default | rejected | rejected | a shift belongs in the transform; a weight on a path is a dissolve |
| raster | default, weight crossfades | rejected | rejected | composition is renderer side |
| ascii grid | default | per-cell density sum, clamped to the ramp | density scale | the glyph is chosen after composition |

`Accumulate` exists for formats that define it across repeats. It is accepted only for numbers, opacity and transforms, and it is evaluated as an add with the one-based offset the format requires.

## 7.5 Partial bindings and masks

Partial bindings are not implemented; when they land, a binding may own part of a property. A transform layer may supply translation only; append puts that translation on the base's rotated and scaled axes, which is what a shake wants, and prepend keeps it in screen space, which a camera move wants and CSS cannot express. A colour layer may supply alpha only, as a slot that leaves RGB untouched. Any other partial binding is rejected.

Three masks exist, and they are different things:

- The time window is a hard mask, one integer compare per sample.
- The weight is the soft mask and the only part that needs a per-frame sample.
- The spatial mask restricts where an effect applies, and only works for per-pixel types: a rectangle or cell range for the ASCII grid, a rectangle or alpha mask for the raster. On a transform, path or scalar it is rejected, because moving half an element is a renderer operation.

## 7.6 Cost

- One layer on one property: zero per-frame cost, because prepare folds it into the single path.
- A stack of n layers: n weight samples plus the combines that are actually active. Closed windows and zero weights are skipped before the value is sampled.
- Constants fold at prepare, so a stack that does not change over a segment costs one combine per frame, not one per key.

## 7.7 Failure modes

- Accumulation is order dependent, so order must be serialised; reordering a JSON array must not change the output.
- A weight outside 0 to 1 is rejected unless the binding declares overshoot.
- A round trip through GIF or Lottie bakes the stack and cannot recover it, so the source timeline stays the only editable copy.
- Double application is the common authoring error: an effect that is already baked into the base must not be added again. The crate cannot detect it, and the importer has no check for a layer-shaped track either; a base timeline carrying one is imported as it stands.
