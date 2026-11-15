# Kinetic motion: craft rules, numbers and segment types

## Purpose

The tween crate interpolates; this note is about making the result read as physical: how far a thing moves between frames, how long a contact lasts, how a bounce decays, how a chain lags, and which spring values become fields.

The scale is 480x270 px, 12 to 25 fps, motions of 4 to 200 px. Sourced numbers carry a reference; (est) marks a craft estimate. The model in `docs/specsheet/04-tween-model.md` and `05-tween-evaluation.md` is assumed: easing on the outgoing key, a spring with an explicit settle, layers by weight.

## Craft rules with numbers

At 12 fps a frame is 83 ms, at 25 fps it is 40 ms, and GIF delays sit on a 10 ms grid with a 20 ms floor.

| fps | ms/frame | 6 frames | 12 frames | GIF delay |
|---|---|---|---|---|
| 12 | 83.3 | 500 ms | 1000 ms | 80 ms |
| 24 | 41.7 | 250 ms | 500 ms | 40/50 alternating |
| 25 | 40.0 | 240 ms | 480 ms | 40 ms |

24 fps is not on the grid: it alternates 40 and 50 ms and a slow pan judders. Delays of 10 ms or less are clamped to 100 ms by browsers, so 20 ms is the floor (`docs/research/multi-conversion-formats-info.md`).

Spacing is timing: the gap between drawings is the speed. A 100 px move over 10 frames at 25 fps, as per-frame deltas:

| curve | deltas in px, frames 1 to 10 |
|---|---|
| linear | 10, 10, 10, 10, 10, 10, 10, 10, 10, 10 |
| decelerate (0, 0, 0.2, 1) | 30.4, 19.6, 14.5, 11.0, 8.4, 6.3, 4.5, 3.0, 1.7, 0.5 |
| CSS ease-out (0, 0, 0.58, 1) | 16.1, 14.8, 13.7, 12.6, 11.4, 10.0, 8.5, 6.7, 4.5, 1.7 |
| in-out (0.4, 0, 0.2, 1) | 2.6, 10.8, 23.3, 24.6, 16.2, 10.0, 6.2, 3.7, 1.9, 0.6 |
| emphasized decelerate (0.05, 0.7, 0.1, 1) | 62.1, 16.3, 8.3, 5.0, 3.3, 2.1, 1.4, 0.9, 0.4, 0.1 |

CSS `ease-out` barely slows the first half, so a 10-frame move still reads near linear. Material's emphasized decelerate puts 62 percent of the travel in frame one, so a 100 px entrance is a pop and a 320 ms creep; use it under 40 px or over 500 ms.

| rule | in frames and pixels | common mistake |
|---|---|---|
| Timing | 4 to 12 frames at 25 fps (160 to 480 ms); a held pose 2 or more. Blink 2 to 3 frames at 24 fps, head turn 6 to 8, take 6 to 10, walk cycle 12 (Williams). | One duration for the scene. |
| Spacing | First and last gap at most 30 percent of the middle: 100 px in 10 frames, middle 20 px, ends 1 to 3. | Linear everywhere. |
| Slow in, slow out | 2 or more frames of deceleration; last frame 0.5 to 2 px, not 8. Start 1 to 2 frames unless already moving. | Single-frame stop. |
| Anticipation | 2 to 4 frames (80 to 170 ms), 5 to 10 percent of travel the other way; heavy 6 to 10, light 2 to 3. | Longer than the action, or none on heavy. |
| Squash and stretch | Contact 15 to 25 percent of height (8 to 12 rigid), stretch 15 to 30 at speed. A 40 px ball to 30-34 tall, 47-53 wide. | No width, or held 3 frames. |
| Follow-through | 1 to 2 frames per link at 25 fps; a 4-link tip lags 4 to 8. | Everything stops together. |
| Overlapping action | 1 to 2 frames per body part (hips 0, chest 1, head 2). | Equal offsets. |
| Arcs | 100 px move bows 5 to 15 px (5 to 15 percent of chord). | Straight-line swing. |
| Secondary action | 10 to 20 percent of primary, 2 to 3 frames behind, off the silhouette. | As large as the primary. |
| Exaggeration | 110 to 130 percent for cartoon, 105 for UI; 2:1 timing contrast does more. | Everything exaggerated equally. |
| Staging | One read per 6 to 8 frames; 2 to 3 px change between frames at 480x270. | Three things at once. |

## Gravity and weight

Fix a scale first. The convention here is 100 px = 1 m (est), so the stage is 4.8 m wide and real gravity is 981 px/s^2.

| gravity | px/s^2 | 40 px drop | 100 px | 200 px | feel |
|---|---|---|---|---|---|
| 0.7x | 687 | 8.5 fr | 13.5 fr | 19.1 fr | floaty |
| 1.0x | 981 | 7.1 fr | 11.3 fr | 16.0 fr | plausible, slightly slow |
| 1.5x | 1472 | 5.8 fr | 9.2 fr | 13.0 fr | cartoon default |
| 2.0x | 1962 | 5.0 fr | 8.0 fr | 11.3 fr | impact |

Frame counts at 25 fps from t = sqrt(2h/g). At 12 fps a drop takes half as many frames, so 40 px is 3 to 4 frames: too few to show acceleration; use anticipation and a contact frame instead.

Gravity's signature is the per-frame step. A 100 px fall at 1.5x and 25 fps steps 1.1, 3.4, 5.6, 7.8, 10.1, 12.3, 14.6, 16.8, 19.0, 21.3 px: each step is 1.1 to 1.3 times the last once under way, and the first is the smallest.

Terminal velocity matters only for light objects: a leaf or balloon settles at 100 to 200 px/s with 2 to 6 px per frame of drift (est). Cap everything else at 800 to 1200 px/s; above that a frame skips a sixth of the canvas.

Mass does not change acceleration: in a vacuum all masses fall alike (Galileo), and in a 270 px scene drag is invisible for anything heavy. Weight is carried by the edges of the fall: anticipation 6 to 10 frames against 2 to 3; contact 2 to 4 against 1, the strongest cue; 0 or 1 bounces against 3 to 5; overshoot 2 percent against 8 to 12; squash 8 to 12 percent against 20 to 30; secondary low and slow against high and fast; a short straight arc against a drifting one.

Cartoon gravity is 1.5 to 2.5 times stronger, has no drag, holds the apex 1 to 2 frames and stops dead with 25 percent squash. Plausible gravity is 1x, drags light objects, has no apex hold, settles 2 to 3 frames and squashes 10 percent.

## Collisions and settling

Restitution is e = v_out / v_in, and successive heights fall by e squared: h(n+1)/h(n) = e^2. The time between bounces falls by e, so the rhythm compresses more slowly than the height, which is why a real bounce sounds like a long gap, a shorter one, then a rattle.

| e | height ratio e^2 | read |
|---|---|---|
| 0.3 | 0.09 | dead, beanbag |
| 0.4 | 0.16 | heavy rubber, 2 bounces |
| 0.5 | 0.25 | classic cartoon ball, 4 bounces |
| 0.55 to 0.65 | 0.30 to 0.42 | natural, 3 to 4 bounces (est) |
| 0.7 | 0.49 | lively, 5 to 6 |
| 0.85+ | 0.72+ | superball, reads as a sine |

A height ratio of 0.35 to 0.55 reads as natural for most objects (est); above 0.7 looks mechanical, below 0.2 looks like sand.

A 100 px drop at 1.5x and 25 fps with e = 0.55 runs 30 frames to rest, over half a 2 s loop; e = 0.4 gives 20 frames and e = 0.7 gives 46. A GIF rarely has room, so cut to two bounces and a settle, or lower e.

Penner's `bounceOut`, behind Godot's `TRANS_BOUNCE` and most CSS bounce presets, is exactly a restitution 0.5 bounce. Its constants (7.5625 and 2.75) put contacts at 0.364, 0.727 and 0.909 of the duration, intervals halving after the first flight, peaks at 0.25, 0.0625 and 0.0156 of the height. A material bounce must expose e, not a fixed curve.

Squash preserves area in 2D: width = 1 / height, so a 20 percent squash widens 25 percent.

A contact frame must exist and be held: 1 frame at 25 fps, 2 for heavy, 1 to 2 at 12 fps. A contact of zero frames reads as a bounce off nothing, the commonest single-frame error in a tweened bounce.

Friction turns a landing into a slide, d = v^2 / (2 * mu * g). At 100 px/m, 400 px/s with mu 0.5 slides 163 px, too far for a 480 px canvas; 200 px/s slides 41 px. Use mu 0.4 to 0.6, v0 200 to 400 px/s, and let the last 20 percent of the distance take 40 percent of the time.

The last pixels of a settle are the tell: the final 2 to 3 px should take 2 to 3 frames at 0.5 to 1 px per frame. A settle that goes 4 px then 0 reads as a snap; 4, 3, 2.2, 1.6, 1.1, 0.7, 0.4, 0.2, 0.1 reads as weight, and 0.5 px is invisible at 480x270.

Stop a spring when the amplitude is below 0.5 px or 0.5 percent of travel, whichever is larger, and the velocity below 2 px/s. Three visible overshoots is the practical maximum; cap the settle at 1.5 times the visual duration and never let a tail cross a cycle's loop point.

## Secondary motion

Follow-through is a delay. A chain of n links at d per link lags the root by n*d. At 25 fps, 1 frame (40 ms) is a tight tail and 2 frames (80 ms) is loose cloth; at 12 fps one frame is already 83 ms, so use fewer links rather than 2-frame delays.

| part | links | delay | z | stiffness | note |
|---|---|---|---|---|---|
| antenna, short tail | 2 to 3 | 40 ms | 0.5 to 0.7 | 300 to 500 | one overshoot |
| long tail, ponytail | 3 to 4 | 40 to 80 ms | 0.35 to 0.5 | 150 to 300 | 2 to 3 swings |
| hair, short | 3 to 4 | 40 ms | 0.3 to 0.45 | 120 to 250 | swings on the beat |
| cloth, hem | 4 to 6 | 40 to 80 ms | 0.25 to 0.4 | 80 to 200 | settles last |
| loose strap | 2 | 80 ms | 0.2 to 0.3 | 100 to 180 | 3 to 4 swings |

A chain settles when z is at least 0.35, stiffness falls 20 to 30 percent per link so the wave travels outward, and gain is at most 0.85. A gain above 1 amplifies the parent and the chain becomes a rubber band; z below 0.25 rings five or more times and reads as jelly, not cloth.

While dragging, the tail points opposite the velocity and lags 1 to 2 frames; when the drag stops, it continues 10 to 15 percent of its length and returns in 3 to 5 frames. The mistake is animating the tail toward the drag direction; it trails behind it.

Overlapping action on a body is a 3 or 4 part chain with 1 frame between parts, and the head's arc is larger than the chest's. Cloth and hair are damped chains of angle springs, not position springs, because a position spring straightens the chain and loses the sag.

## Springs and curves

A spring is stiffness k, damping c and mass m, or the two numbers that control feel: damping ratio z = c / (2*sqrt(k*m)) and natural frequency w = sqrt(k/m). The ratio sets overshoot, the frequency sets speed.

| source | k | c | z | overshoot | settle | frames at 25 |
|---|---|---|---|---|---|---|
| Motion default | 100 | 10 | 0.50 | 16.3% | 1.07 s | 27 |
| Motion bounce 0.25 | derived | derived | 0.75 | 2.8% | 0.55 s | 14 |
| react-spring default | 170 | 26 | 1.00 | 0% | 0.61 s | 15 |
| react-spring wobbly | 180 | 12 | 0.45 | 20.8% | 0.90 s | 23 |
| react-spring stiff | 210 | 20 | 0.69 | 5.0% | 0.56 s | 14 |
| Material expressive fast spatial | 800 | 34 | 0.60 | 9.5% | 0.33 s | 8 |
| Material expressive default spatial | 380 | 17 | 0.80 | 1.5% | 0.37 s | 9 |
| SwiftUI .bouncy | 158 | 17.6 | 0.70 | 4.6% | 0.43 s | 11 |
| UIKit default | 1000 | 500 | 4.56, behaves 1.0 | 0% | 0.34 s | 9 |

Values from Motion (`packages/motion-dom/src/animation/generators/spring.ts`), react-spring `config`, Material `ExpressiveMotionTokens.kt`, SwiftUI `Spring`. Settle is computed to a 0.5 percent band from the underdamped envelope; overshoot is exp(-pi*z/sqrt(1-z^2)).

z 0.2 to 0.35 is jelly and should not ship; 0.4 to 0.55 is playful with two or three bounces; 0.6 to 0.75 is confident, one bounce and 3 to 9 percent; 0.8 to 0.95 is crisp under 2 percent; 1 or above never overshoots. Frequency sets pace: w 8 to 12 rad/s is slow and heavy, 15 to 25 is normal UI, 30 to 40 is snappy.

The Motion default: 100/10 overshoots 16 percent and takes 1.07 s, 27 frames at 25 fps, over a third of a 3 s loop for one move. Material's 800/34 settles in 8 frames with 9.5 percent.

A spring starts and ends with velocity, so consecutive springs join smoothly and an interruption can retarget. A cubic bezier has zero velocity at both ends unless the handles are set, so two bezier segments with different end slopes jump in velocity at the key; `docs/research/tween-math-easing.md` notes position is continuous but velocity is not.

Use a spring when the motion is a reaction: a landing, a settle, a follow-through, an interruption. Use a bezier when it is choreography: an entrance, an exit, a camera move, anything that must hit a mark on a beat or close a loop. A bezier also wins when the frame budget is fixed, because its duration is authored and a spring's is emergent.

Godot's taxonomy is the cross-check. `TRANS_BACK` overshoots exactly 10 percent at its peak with the constant 1.70158; `TRANS_ELASTIC` at Penner's amplitude 1 and period 0.3 overshoots 59 percent on the first swing, which is why it reads as jelly; `TRANS_BOUNCE` is the e = 0.5 series. Quad, cubic, expo and sine are bezier presets, not new evaluators. Worth naming: back and bounce; elastic is a spring at z around 0.2 and should be spelled that way.

## Kinetic type and small motion

Overshoot on type is 2 to 5 percent of travel for UI and 8 to 12 for a playful read (est). The pixel cap matters more: 5 percent of 40 px is 2 px and reads as a bounce, 5 percent of 200 px is 10 px and reads as a mistake. Cap at 6 px regardless of distance.

A cubic bezier does not give the overshoot you ask for. Setting the first y control to 1.05, the naive way to write 5 percent, produces 0.04 percent. The relationship is steep: y1 1.2 gives 1.25, 1.3 gives 3.0, 1.4 gives 5.3, 1.5 gives 8.0, 1.7 gives 14.3. Material's expressive curve (0.42, 1.67, 0.21, 0.9) gives 9.2. Author overshoot by evaluating the curve, not by writing a percentage into a control point.

Smear frames are 1 to 2 frames at 25 fps (40 to 80 ms): a stretched copy along the path at 40 to 60 percent opacity, or two to four offset copies, which is how motion blur is faked without a shutter. At 12 fps a smear lasts 83 ms and reads as a deliberate multiple, not blur, so smears belong to the 25 fps end. Multiples are cheaper: two or three copies at 30 to 50 percent, offset by the per-frame travel.

On twos, 12 drawings per second at 24 fps, each holds two frames; on ones every frame is new. 12 fps reads as drawn because apparent motion needs 10 to 12 samples per second and each pose is held long enough to be seen; 24 fps reads as smooth because 42 ms is near the limit at which the eye separates images. A 12 fps move with 20 px between frames is a jump; a 25 fps move with 2 px is a glide.

Pixel snapping is a small-motion problem: snapping a 4 px move over 10 frames, at 0.4 px per frame, turns it into four 1 px jumps. Snap at render for text and strokes, keep sub-pixel for filled shapes, and never snap inside the tween.

## Encoding in a keyframe format

The crate's `Ease` returns progress in 0 to 1 between two keys. A ballistic segment does not: it owns an absolute value and ignores the end key, because the end of a fall is a floor, not a key. The addition is therefore a segment kind that produces a value, with the existing easing as the default case.

```rust
pub enum Motion {
    Ease(Ease),                 // progress 0..1 between key values
    Ballistic(Ballistic),       // absolute value, ignores the end key
    Arc { bend: f64, ease: Ease },
    Chain(Chain),               // reads another track, delayed
}
```

A ballistic segment compiles at prepare into flights and contact holds, so `sample` stays a lookup plus a closed form. Each flight is y = y0 + v0*t + 0.5*g*t^2 with a solved contact time; a hold is a zero-length flight at the floor.

| field | unit | meaning | default |
|---|---|---|---|
| `g` | px/s^2 | gravity on the value axis, positive down | 1400 |
| `v0` | px/s | initial velocity, signed | 0 |
| `plane` | px | value at which contact occurs | required |
| `restitution` | 0 to 1 | speed ratio at contact | 0.5 |
| `friction` | 0 to 1 | horizontal loss per contact | 0.3 |
| `min_bounce` | px | stop when the rebound is below this | 1.0 |
| `max_speed` | px/s | terminal speed cap | 1000 |
| `contact_hold_us` | us | hold at the floor after contact | 40000 |
| `squash` | fraction | height lost at contact | 0.2 |
| `stretch` | fraction | length gained at peak speed | 0.15 |

An arc is a quadratic through a control point offset from the chord midpoint by `bend` times the chord, signed; 0.1 bows a 100 px move by 10 px.

A chain is better as a layer than a segment, because it reads another track. Fields: `links` (2 to 6), `delay_us` (40000 at 25 fps), `stiffness`, `damping`, `mass`, `gain` (0.85), `taper` (0.75, the stiffness multiplier per link). It compiles to n delayed spring evaluations and fits part 07 as an additive binding with a weight.

Two spring fields are missing: `rest_delta` in px (0.5) and `rest_speed` in px/s (2). The explicit settle time stays; these decide when it is reached, and without them the settle cannot be reproduced from the parameters.

Worth adding: ballistic, the only way to get a real bounce and one page of closed-form code; arc bend, nearly free; chain, as a layer modifier; rest thresholds. Not worth adding: smear and blur, which are renderer work, and squash, which belongs on a deform track driven by speed.

## Failure modes

| symptom | cause | fix |
|---|---|---|
| Floaty | too long, gravity too low, no contact | cut 30 percent, g 1400, 1-frame contact |
| Ice skating | constant-rate translation, no plant | hold the contact, decelerate, mu 0.4 to 0.6 |
| Jitter | two layers fighting, or a spring near rest | one owner per property, rest_delta 0.5 px |
| Never settles | z below 0.3, no settle cap | z 0.5 or above, max 3 overshoots |
| Contradicts mass | elastic on heavy, slow ease-in on light | z 0.8 to 1.0 heavy, 0.4 to 0.6 light |
| Too smooth | no holds, everything overlapping | 1 to 2 frame holds, 2 to 3 px per frame |
| Bounce like a sine | equal heights or equal intervals | heights e^2, intervals e, cut the last |
| Pixel crawl | rounding to integers every frame | snap at render only |
| Loop seam | a spring or chain still moving | settle before the end |

## Defaults for this project

Frames at 25 fps; at 12 fps use half as many for the same milliseconds, which is often too few, so prefer the long end of each range.

| case | travel | duration | frames | segment or easing | parameters | follow-through |
|---|---|---|---|---|---|---|
| Entrance | 4 to 60 px | 240 to 320 ms | 6 to 8 | (0, 0, 0.2, 1) | overshoot 2 to 4%, y1 1.3 | 1 link, 40 ms |
| Exit | 4 to 60 px | 120 to 200 ms | 3 to 5 | (0.4, 0, 1, 1) | no overshoot | none |
| Drop | 40 to 200 px | 240 to 520 ms (fall) | 6 to 13 | ballistic | g 1400, e 0.5, 2 bounces | squash 20% |
| Bounce | 8 to 40 px | 320 to 400 ms | 8 to 10 | spring | k 400, c 30, z 0.75 | 1 link, 40 ms |
| Slide | 40 to 200 px | 320 to 480 ms | 8 to 12 | decelerate + friction | v0 200 to 400 px/s, mu 0.5 | 1 frame tail |
| Shake | 2 to 4 px | 120 to 200 ms | 3 to 5 | sine or spring layer | 4 to 6 cycles, decay 0.5 | weight fade |
| Pulse | 1.00 to 1.05 | 320 to 400 ms | 8 to 10 | (0.4, 0, 0.2, 1) | 2 to 5%, no bounce | none |
| Camera | 100 to 480 px | 800 to 1600 ms | 20 to 40 | linear or (0.4, 0, 0.2, 1) | no overshoot | parallax 2 to 6 px |

## Checklist

- Every move has a frame count, not only a duration.
- First and last gaps are at most 30 percent of the middle gap.
- Steps grow 1.1 to 1.3 in a fall and shrink 0.7 to 0.9 in a settle.
- Every contact exists as a frame and is held at least one frame.
- Bounce heights decay by e^2, intervals by e.
- Squash preserves area: width = 1 / height.
- Heavy means longer anticipation and contact, fewer bounces, less overshoot, not a faster fall.
- Overshoot is capped at 6 px and measured from the evaluated curve.
- Every spring names a rest delta, a rest speed and a maximum settle.
- Chains have z at or above 0.35 and gain at or below 0.85.
- No spring or chain is still moving at the loop point.
- Sub-pixel positions survive the tween and snap only at render.

## References

Books: Richard Williams, *The Animator's Survival Kit*, timing, spacing and the bouncing ball; Frank Thomas and Ollie Johnston, *The Illusion of Life*, the twelve principles.

Documentation: Material Design 3 motion (`m3.material.io/styles/motion`); Motion springs (`motion.dev/docs/spring`), source `packages/motion-dom/src/animation/generators/spring.ts`; SwiftUI `Spring` (`developer.apple.com/documentation/swiftui/spring`); Godot `Tween`.

Repository: `docs/research/tween-math-easing.md`, `docs/specsheet/05-tween-evaluation.md`, `docs/specsheet/07-tween-composition.md`, `docs/research/multi-conversion-formats-info.md`, `docs/specsheet/01-behaviour-baseline.md`. Frame counts, the 100 px/m scale, the restitution band, chain parameters and pixel caps are estimates to tune against rendered frames.
