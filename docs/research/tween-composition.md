# Tween composition: layers, weights and composite operations

## Purpose

A sampler returning one value per track cannot express a shake added to a base, a pulse multiplied into a scale, or an effect that fades in. This note specifies the layer stack above `tween-timeline-model.md`: operations, per-type arithmetic, ordering, masks, cost and tests, with one clip per property costing the same as today.

## Semantics borrowed from formats

Web Animations puts an effect stack on each property, combines effects in composite order with each result the underlying value for the next, and takes the last as the composited value (web-animations-1 5.4.2, 5.4.3). Replace returns the effect value, add adds it, accumulate accumulates it, underlying before effect, and order is class, class rule, then global list (5.4.4, 5.4.2); a missing 0 or 1 keyframe is synthesised from the underlying value with replace (5.3.4).

`iterationComposite: accumulate` accumulates each iteration with the final value of the previous one (web-animations-2 3.4), combining the final keyframe value with the current one through the property's accumulation procedure (3.2.1 step 12.2). A type without one returns the second operand, so the value holds.

CSS Values 4 defines addition and accumulation on computed values, and where a type is not additive "its addition operation is simply Vresult = VB" (3); a type without an accumulation procedure accumulates as it adds. Range checking follows all three operations (3.1), and colours are not additive (8.1.1).

Transform addition is concatenation, Vb appended to Va, while accumulation matches functions, pads with identities and adds parameters, except one-based ones such as scale and the matrix diagonal, which use Va + Vb - 1 (css-transforms-2 15). The neutral element for addition is 0, so scale(0) is neutral rather than identity (15.1).

`animation-composition` exposes replace, add and accumulate per animation, default replace, sticky per property (css-animations-2 4.10), and has no weight.

SMIL supplies the repeat rule: `additive="sum"` adds to the underlying value and to lower-priority animations, `replace` overrides them, priority is begin time then document order (smil-animation 3.3.6, 3.5), and `accumulate="sum"` gives fi(t) = ve * i + f(t) with ve the last value specified (3.3.1). Its from-to example reads as if ve were the delta, reaching 110 after ten repeats where the formula gives 200; Web Animations adds the final value, so the formula is the rule.

Table 1. What each format settles.

| Format | Stack and order | Operations | Weight over time | Not addable |
|---|---|---|---|---|
| Web Animations | Per property; class, class rule, global list | replace, add, accumulate, per effect and per key | None; a second effect with add is the only fade | Falls back to replace |
| CSS animations | Per property; owning element tree order, then name position | replace, add, accumulate, sticky per property | None | Replace |
| SMIL | Per attribute; begin time, then document order | replace or sum; accumulate across repeats | None; fill and freeze only | "Cannot support additive animation"; accumulate ignored |
| Lottie | No stack; one value per property per frame | None | None | Not applicable |

A missing base cannot occur in CSS, where every property has a computed value; here it can, so the base is declared. SMIL takes it from the object model and gives no effect when the host lacks the attribute (3.1), and a zero base is different: the neutral element of transform addition is 0, so adding to nothing yields a degenerate matrix.

On a non-addable type CSS replaces, showing a plausible wrong picture, while SMIL says such attributes "cannot support additive animation" and leaves the result undefined (3.3.6), ignoring accumulate (3.3.1); players replace in practice (unverified). An offline renderer should not copy the silent branch.

## Prior art in tools

Blender's NLA evaluates tracks bottom-up: each strip blends with "the tracks below" (manual, NLA Sidebar), so a higher Replace strip wins. Modes are Replace, Multiply, Subtract, Add and Combine, with influence 0..1 that is keyframable and rampable at strip edges. Combine is type-aware, multiplying scale by (value/default)^influence and adding (value - default) times influence elsewhere, which makes it rest-relative while Add is a plain sum; extrapolation is Hold, Hold Forward or Nothing, and strips may share one action (NLA Strips).

Unreal splits the delta by space: an additive sequence declares Additive Anim Type Local or Mesh Space and a Base Pose Type, and mesh space keeps an aim offset pointing where its rotation says while local space skews it (Aim Offset). Application is a node with alpha 0..1 (AnimNode reference). Montages deliver one-shots through slots that override by default, and additive montages need an additive sequence (Animation Montage, unverified).

Unity's layers have weight, Override or Additive, and an avatar mask, but require an additive layer to contain every property of the layers below (Animation Layers); a translation-only layer is unsupported there and is the main case here.

GSAP has no stack: `overwrite: false` lets tweens fight, `true` kills every tween of the target, `"auto"` kills only conflicting properties, and the last writer wins. anime.js v4 offers replace, blend and none, blending additively while both play forward; Rive's Additive blend state stacks timelines with an amount 0..100 from a value or input (State Machine, States); Lottie has none.

Table 2. What the tools add.

| Tool | Stack | Operations | Weight | Notable limit |
|---|---|---|---|---|
| Blender NLA | Tracks bottom-up, strip blends with the result below | Replace, Add, Subtract, Multiply, Combine | Influence 0..1, keyframable, blend in/out | Combine is type-aware; Add is a plain sum |
| Unreal | Graph nodes; base plus additive pose; montage slots | Additive in local or mesh space | Alpha 0..1, float, bool or curve | Sequence must be authored additive, with a base pose |
| Unity | Layers, Override or Additive | Replace or add | Layer weight | Additive layer must contain the same properties |
| GSAP | None | Kill the conflicting tween | None | Last writer wins |
| anime.js | None | replace, blend, none | None | Blend only while both play forward |
| Rive | State layers and states | Additive blend of timelines | Amount 0..100 per timeline | Additive only, inside a state |
| Lottie | None | None | None | One value per property |

## Model for this crate

A clip is a timeline of tracks, unchanged. A layer is a named set of bindings plus a weight track, a time window and an optional mask; a binding is one layer's contribution to one property. The stack is per property, the unit Web Animations composites.

Order is explicit and total: an integer `order`, sorted on, duplicates rejected at prepare; a tie-break by id would change output when ids change and one by array position when a JSON array is rewritten.

Weight applies to the result, not the value: `v = mix(v, combine(v, x), w)`. Every operation is identity at w = 0 and exact at w = 1 by construction, including multiply and transform concatenation, where scaling the value would not scale the effect; at w = 1 the mix is skipped, so endpoints stay exact. The weight is itself a track, so a fade is two keys.

```rust
pub struct LayerId(u32);
pub struct PropId { pub element: u32, pub slot: u16 }

pub struct Layer {
    pub id: LayerId, pub order: u32,
    pub window: Option<Window>,   // hard time mask, ms
    pub weight: Track,            // scalar, layer default
    pub mask: Option<Mask>,       // per-pixel types only
    pub bindings: Vec<Binding>,
}

pub struct Binding {
    pub target: PropId, pub op: Op, pub track: Track,
    pub weight: Option<Track>,    // overrides Layer::weight
    pub mode: AddMode,            // transform only
    pub overshoot: bool,          // permit w outside [0,1]
}

pub enum Op { Replace, Add, Multiply, Accumulate }
pub enum AddMode { Append, Prepend }
pub enum Mask { Rect(Rect), Cells(Range<u32>), Alpha(RasterRef) }

pub enum PropBody {
    Single(PreparedTrack),             // zero or one contributing binding
    Stacked { base: Base, layers: Range<u32> },
}

pub struct PreparedLayer {
    pub op: Op, pub mode: AddMode,
    pub weight: PreparedTrack, pub value: PreparedTrack,
    pub window: Option<Window>, pub clamp_w: bool,
}

pub enum Base { Neutral, Constant(Value), Clip(PreparedTrack) }
```

`Single` is the current code path, unchanged and uninstrumented: one Replace binding gives a `Single` over its track with a constant weight folded into the keys, and `Stacked` indexes a contiguous arena, so a stack is one range and sampling allocates nothing. Sampling walks that range, skipping closed windows and zero weights, sampling the value only when w > 0, and taking `combine` exactly at w = 1.

Prepare resolves ids, masks and windows; groups bindings by `order` and rejects duplicates; drops zero weights and empty windows; folds constants, so a Replace binding with constant value and weight 1 replaces the base and a constant Add or Multiply folds into one layer; gives `Single` for zero or one contributing Replace binding and `Stacked` otherwise; and sets `clamp_w` only when the weight can leave [0,1], which a bezier with y handles inside [0,1] cannot. `Base::Neutral` is 0, 1, the identity matrix, transparent black, the empty path or the empty grid by type; no base and no Replace binding is a prepare error.

### A layer that owns only part of a property

A transform binding may supply a subset of functions. `Append` concatenates, matching CSS, so a translation-only layer shakes along the base's rotated and scaled axes; `Prepend` keeps it in screen space, which a camera move wants, and is an extension because CSS has no prepend. A colour binding may supply alpha only, as an `Alpha` slot that leaves RGB untouched; other partial bindings are rejected.

### Masks

The time window is a hard mask, skipped by one integer compare; the weight is the soft mask and the only part needing a per-frame sample. The spatial mask restricts where the effect applies and only works for per-pixel types, a rect or cell range for the ASCII grid and a rect or alpha mask for the raster; on a transform, path or scalar it is rejected, because moving half an element is a renderer operation. The sampler carries it into the scene; it does not apply it.

## Per-type rules

Table 3. Operation support. "Neutral" is the value the operation treats as no change.

| Type | Replace | Add | Multiply | Accumulate | Notes |
|---|---|---|---|---|---|
| number | yes | plain sum | product | same as Add, neutral 0 | Clamp only if the property declares a range |
| opacity | yes, default | sum, then clamp to [0,1] | product | as Add | Add saturates; independent overlays need Multiply |
| transform | yes | list concatenation (CSS) | matrix product | component-wise, Va + Vb - 1 for one-based | Non-commutative; Add with scale, matrix or perspective rejected |
| colour | yes, default | RGB sum scaled by layer alpha | premultiplied channel product | as Add | CSS says colour is not additive; Add fails unless allowed |
| path | yes | rejected | rejected | rejected | A shift belongs to the transform |
| raster | yes, weight gives a crossfade | rejected | rejected | rejected | Composition is renderer-side |
| ASCII grid | yes | per-cell density sum, clamped to the ramp | per-cell density scale | as Add | Glyph identity is not addable; the glyph is chosen after composition |

Colour has no reference behaviour, so the crate defines it. Add works in premultiplied space, affecting RGB only and scaling the layer's colour by its own alpha, leaving alpha to Replace or Multiply; adding alpha too would turn two opaque layers into alpha 2 and a darker result after unpremultiplication. A transparent layer then contributes nothing, and both operands are converted to the timeline's declared space first, because gamma sRGB and linear light give different midpoints.

Transform Add is rejected when the list contains scale, matrix or perspective: concatenating two scales multiplies them, so a weight ramp slides the base scale to the product instead of by a fraction of a delta, and the same operation means addition for translate and multiplication for scale. Accumulate resolves that by using the neutral element per parameter.

Path, raster and glyph identity are replace-only: a path has no addition that preserves its structure, a raster is a source reference plus a resampling plan so blending belongs to the renderer, and an ASCII cell's density composes while its glyph identity does not. When an add is requested on a type that cannot add, prepare fails with the layer, property, operation and type; a lenient mode may downgrade to Replace but must report it.

## Cost

Per binding per frame: one window compare (about 1 ns), one weight sample (1 to 2 ns scalar, 10 to 20 ns bezier), one value sample at the existing cost, one combine (1 ns scalar, about 20 ns colour, 50 ns transform) and one mix at the same order; a Replace binding with constant weight folds away at prepare.

A two-layer transform property costs roughly 120 ns against 50 ns today, and four hundred of them go from about 20 us to 48 us per frame, under a tenth of the cheapest raster row in `tween-performance-memory.md`. Memory is one `PreparedLayer` per binding, 64 to 96 bytes, sized at prepare, and `PropBody::Single` never enters the loop, never reads a weight and never calls `combine`.

## Failure modes

**Order dependence.** Add commutes for scalars but not transforms, and clamping makes even Multiply non-associative. With Append, `rotate(90) translate(10,0)` moves along the rotated axis while `translate(10,0) rotate(90)` moves along the screen axis, so the order field must be serialised and total and a shuffled array with unchanged order values must produce identical output.

**Non-commutative accumulation.** Accumulate over three layers depends on the order of the first two for the same reason, and A then B is not B then A whenever either carries a rotation or translation; one concrete triple is pinned so a refactor cannot quietly reorder it.

**Weight outside [0,1].** A weight is a mix factor, so keys are validated to [0,1], and a curve that can overshoot between keys is clamped only when it can, with `overshoot: true` turning the clamp off. This is not the CSS clamp-the-final-value rule: 1.5 multiplies a delta by 1.5, a different effect, not an extrapolated one.

**Missing ids.** A binding, window or mask naming an absent layer, element or asset fails the load, because skipping it would render a plausible animation with one effect missing, the worst failure for an offline render.

**Double application after a round trip.** A format with no layer concept flattens the stack into per-frame values, and re-importing that as a base and re-attaching the layers doubles the effect. A baked export records `baked_layers: [LayerId]` and `composition: "baked"`, prepare rejects a stack that re-applies a layer in the base's `baked_layers`, and the importer never re-attaches by id. Baking quantises to the frame grid, so a round trip changes output regardless, and the bake reports `Fidelity::Baked` with the maximum error over sampled frames.

## Tests

1. `weight_zero_is_identity`: for every operation and type, constant weight 0 leaves the base bit-identical.
2. `three_layer_sum`: three Add bindings of 0.1 on 0.2 give 0.5 within 1e-15, in any array order.
3. `three_layer_transform_order`: base `translate(10,0)`, A `rotate(90)`, B `translate(10,0)`; Append rotates the translation, Prepend keeps it on screen, and swapping A and B changes both.
4. `clamp_after_stack`: base 1.5, Add 0.5, Multiply 0.5 gives 0.5 after a final clamp to 1, not 0.75.
5. `reversal`: a stack with the list reversed equals the mirrored timeline at -t when the layers commute, and differs as documented when they do not.
6. `duplicate_order_rejected`, `missing_layer_rejected`, `add_on_path_rejected`, `add_on_colour_rejected_unless_allowed`, `mask_on_transform_rejected`.
7. `shuffled_input_is_stable`: another array order with unchanged `order` values gives identical output.
8. `single_layer_compiles_to_single`: a one-layer Replace property produces `PropBody::Single`, and a counter shows zero `combine` calls over a full render.
9. `baked_round_trip_detected`: a base whose `baked_layers` contains a re-applied layer fails prepare with `DoubleApplication`.

## References

- Web Animations 1 (5.3.4, 5.4.2, 5.4.3, 5.4.4): https://drafts.csswg.org/web-animations-1/ ; Web Animations 2 (3.2.1, 3.4, 4.11): https://drafts.csswg.org/web-animations-2/
- CSS Values 4 (3, 3.1, 8.1.1): https://drafts.csswg.org/css-values-4/ ; CSS Transforms 2 (15, 15.1): https://drafts.csswg.org/css-transforms-2/ ; CSS Animations 2 (4.10): https://drafts.csswg.org/css-animations-2/
- SMIL Animation (3.1, 3.3.1, 3.3.6, 3.5): https://www.w3.org/TR/smil-animation/
- Blender manual, NLA Strips and Sidebar: https://docs.blender.org/manual/en/latest/editors/nla/strips.html , https://docs.blender.org/manual/en/latest/editors/nla/sidebar.html
- Unreal, Aim Offset and Animation Montage: https://dev.epicgames.com/documentation/en-us/unreal-engine/aim-offset-in-unreal-engine , https://dev.epicgames.com/documentation/en-us/unreal-engine/animation-montage-in-unreal-engine ; AnimNode reference: https://www.aaronkemner.com/animnode-reference/applymeshspaceadditive/
- Unity Animation Layers: https://docs.unity3d.com/Manual/AnimationLayers.html ; GSAP conflicting tweens: https://gsap.com/resources/conflict/ ; anime.js composition: https://animejs.com/documentation/animation/tween-parameters/composition/
- Rive State Machine States: https://rive.app/docs/editor/state-machine/states ; lottie-spec `docs/specs/properties.md`: https://github.com/lottie/lottie-spec
- Repo docs: `docs/research/tween-timeline-model.md`, `tween-math-easing.md`, `tween-property-types.md`, `tween-performance-memory.md`

Unverified: whether SMIL players replace or ignore when `additive="sum"` meets a non-addable attribute; how Unreal applies an additive montage through a slot beyond requiring an additive sequence; whether Rive applies the additive amount before or after blending state layers; and browser support for `iterationComposite`.
