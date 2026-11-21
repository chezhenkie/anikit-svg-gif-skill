# 10-kinetic-motion

Status: implemented for ballistic and arc motion on scalar tracks. Friction, squash and stretch are not implemented, and a scene that names one is refused until a renderer draws the deformation. Surfaces and deformation are rules for drawn or baked art; the mesh kind is not implemented. Evidence and sources: docs/research/kinetic-motion.md. This part turns that report into rules the crate and the routes must satisfy.

## 10.1 Why these rules

The research changed four things about how the tween crate is designed.

- Weight is carried by the edges of a move, not the middle. Gravity is the same for every mass, so a heavy object does not fall faster; it anticipates longer, holds contact longer, bounces less and overshoots less. A crate that only offers easing cannot express that, which is why a ballistic segment and a contact hold are needed.
- What a viewer reads is spacing, the gap between frames, not the curve name. Two curves with the same name can produce very different deltas, so the tests must assert delta profiles, not curve shapes.
- A spring is a reaction and a bezier is choreography. Springs are right for landings, settles and follow-through; beziers are right for entrances, exits, camera moves and anything that must close a loop. A spring's duration is emergent, which collides with a fixed frame plan, so its settle time has to be computed and capped.
- The frame budget is the real constraint. At 25 fps a two second loop is 50 frames, and one spring at Motion's 1.07 s physics fallback eats 27 of them; the crate's 400 ms default uses 10. Defaults must be short, and a chain or a spring that is still moving at the loop point is a defect.

## 10.2 Motion vocabulary

A segment produces a value over a span of time. Easing is the default case; two more kinds exist.

```rust
pub enum Motion {
    Ease(Ease),
    Ballistic(Ballistic),
    Arc { bend: f64, ease: Ease },
}
```

- `Ease` interpolates between two key values with progress 0 to 1, as specified in part 05.
- `Ballistic` owns an absolute value and ignores the end key, because a fall ends on a plane, not on a key. It compiles at prepare into flights and contact holds, so sampling stays a closed form.
- `Arc` is a quadratic through a control point offset from the chord midpoint by `bend` times the chord length. A quadratic reaches half that offset at its midpoint, so a bend of 0.1 bows a 100 px move by 5 px.
- A follow-through chain is not a segment. It should read another track through a layer binding with a delay, which part 07 defines, but a binding has no delay field yet: today a chain is written as a copied track with its keys shifted later. A copy clamps at the source end, so in cycle mode it must wrap modulo the duration, and a copied track has no spring of its own.

Ballistic fields, all compiled at prepare:

| field | unit | meaning | default |
|---|---|---|---|
| `gravity` | px/s^2 | acceleration on the value axis, positive down | 1400 |
| `v0` | px/s | initial velocity, signed | 0 |
| `plane` | px | value at which contact occurs | required |
| `restitution` | 0 to 1 | speed ratio kept at contact | 0.5 |
| `min_bounce` | px | stop when the rebound is below this | 1.0 |
| `max_speed` | px/s | terminal speed cap | 1000 |
| `contact_hold_us` | us | hold at the plane after contact; set it to one frame at the scene rate | 40000 |

A spring gains two fields, because a settle cannot be reproduced from stiffness and damping alone: `rest_delta` in pixels (0.5) and `rest_speed` in px/s (2). The explicit settle time stays, and a settle longer than 1.5 times the visual duration is capped at that length rather than refused. Under `once` the cap is harmless, because motion past the segment is not sampled. Under `cycle` the same motion crosses the seam, so a spring must reach rest by the segment end; the cap does not enforce that, so the author does.

## 10.3 Contact, bounce and settle

- Every contact reads as a held frame, but the plan has no contact-time hook yet: a 40 ms contact is one frame at 25 fps, and at 12 fps one frame is 80 or 90 ms, so the hold is max(40 ms, one frame) and its key sits on a frame time. A contact of zero frames reads as a bounce off nothing, and it is the commonest single error in a tweened bounce.
- A rest position inside a loop holds for at least 3 frame times, 120 ms at 25 fps and 250 ms at 12; one frame reads as a pause. The plan shows its last sample through the tail, so the pose must be settled by that sample.
- Successive bounce heights fall by the square of restitution and the intervals fall by restitution; the sequence is geometric in time and height, never a linear spacing. A height ratio between 0.35 and 0.55 reads natural, which is restitution 0.59 to 0.74; the cartoon default of 0.5 gives 0.25. Above 0.7 it looks mechanical; below 0.2 it looks like sand.
- A GIF has no room for a full decay. A 100 px drop at restitution 0.55 runs 30 frames to rest at gravity 1400 and a 1 px minimum bounce, so cut to two bounces and a settle, or lower restitution to 0.4.
- Squash preserves area in two dimensions: width = 1 / height. A 20 percent squash widens 25 percent. Contact squash is 15 to 25 percent, stretch at speed 15 to 30 percent. The compressed face is the contact face: the bottom on a floor, the side on a wall, the top on a ceiling, with the flattening along that normal. Squash and stretch are not implemented; a ballistic key that names either is refused, so draw the deformation into the art or bake the frames.
- The face rule is drawn in [contact-face.png](../../agent-skill/references/contact-face.png): a floor, a wall, a ceiling and an angled surface, each free, in contact and recovering. The face is flat along the contact normal and the body bulges away from the surface so the area is kept, whatever the shape. A bounce is a run of these contacts, each flattening the face and restoring it before the next, and a pose swap starts with the face already flat.
- The last pixels are the tell. The final 2 to 3 px take 2 to 3 frames at 0.5 to 1 px per frame. A move that goes 4 px then 0 reads as a snap.
- A slide decelerates with friction `mu`, but friction is refused. Constant deceleration is exactly `cubic-bezier(0.333, 0.667, 0.667, 1)`: for distance d, T = sqrt(2d/(mu g)) and v0 = 2d/T. At mu 0.5 and gravity 1400, 100 px needs 374 px/s over 535 ms, and the last 20 percent of the distance takes 45 percent of the time.

## 10.4 Weight

Weight is not speed. These are the cues that carry it, light against heavy:

| cue | light | heavy |
|---|---|---|
| anticipation | 2 to 3 frames, 80 to 120 ms | 6 to 10 frames, 240 to 400 ms |
| contact hold | 1 frame, 40 ms | 2 to 4 frames, 80 to 160 ms |
| bounces | 3 to 5 | 0 to 1 |
| overshoot | 8 to 12 percent | 2 percent |
| squash at contact | 20 to 30 percent, drawn | 8 to 12 percent, drawn |
| secondary motion | high and fast | low and slow |
| arc | drifting | short and straight |
| damping ratio | 0.4 to 0.6 | 0.8 to 1.0 |

Gravity is a scene setting, not a per-object one. A scene uses 1400 px/s^2 for a cartoon read, 981 px/s^2 for a plausible one, and holds one value throughout, because two objects falling at different rates in the same shot read as an error. At 12 fps hold the milliseconds and halve the frame counts: five frames there is 400 ms, not 200.

## 10.5 Springs and curves

- Damping ratio sets overshoot, natural frequency sets pace. 0.4 to 0.55 is playful, 0.6 to 0.75 is confident, 0.8 to 0.95 is crisp, 1.0 and above never overshoots. Frequency 8 to 12 rad/s is heavy, 15 to 25 is normal, 30 to 40 is snappy.
- Use a spring for a reaction: a landing, a settle, a follow-through, an interruption. Use a bezier for choreography: an entrance, an exit, a camera move, a loop.
- A bezier's duration is authored and a spring's is emergent, so a route with a fixed frame count prefers a bezier.
- Overshoot is not a control point value. With the second y handle at 1, writing 1.05 into the first produces 0.04 percent, not 5; y near 1.4 gives about 5.3 percent. The crate therefore measures overshoot from the evaluated curve, and the sampled value passes the end key, so the overshoot is drawn. A 6 px cap on that overshoot is not implemented yet. When it is, it applies at the furthest point of the element and to every property: at radius r, a scale of 6/r and a rotation of 6/r radians stay inside it. Until then keep travel small.

## 10.6 Frame rate and small motion

- 12 fps reads as drawn because each pose is held long enough to see; 25 fps reads as smooth. 24 fps is not on the 10 ms grid: it gives four 40 ms frames and one 50 ms frame in every group of six, so it is not a target.
- At 12 fps the delay runs 80, 90, 80 in each 250 ms, 83 on average, and the frame count closes the loop on the grid; use fewer links in a chain rather than longer delays.
- Positions stay sub-pixel through the tween and are snapped only at render, for text and strokes. Snapping inside the tween turns a 0.4 px per frame move into four 1 px jumps.
- A smear belongs at 25 fps, where it lasts 40 to 80 ms: one frame carrying two or three copies at 30 to 50 percent opacity, offset by the previous frame's delta. No field or renderer path exists, so draw it into the art or bake it. At 12 fps it reads as a deliberate multiple.

## 10.7 Defaults

Frames are at 25 fps; at 12 fps hold the durations and halve the frame counts.

| case | travel | duration | segment | parameters | available |
|---|---|---|---|---|---|
| entrance | 4 to 60 px | 240 to 320 ms | bezier (0, 0, 0.2, 1) | 2 to 4 percent overshoot as a key, one link 40 ms | yes |
| exit | 4 to 60 px | 120 to 200 ms | bezier (0.4, 0, 1, 1) | none | yes |
| drop | 40 to 200 px | 240 to 540 ms | keyed fall or ballistic scalar | g 1400, cartoon e 0.5, two bounces | yes |
| bounce | 8 to 40 px | 320 to 400 ms | spring | k 400, c 30, z 0.75, one link 40 ms | yes |
| slide | 40 to 200 px | T = sqrt(2d/(mu g)), 340 to 760 ms | cubic-bezier(0.333, 0.667, 0.667, 1) | mu 0.5, g 1400, v0 = 2d/T, one frame tail | ease only |
| shake | 2 to 4 px | 120 to 200 ms | spring layer | 4 to 6 cycles, decay 0.5, weight fade | yes |
| pulse | scale 1.00 to 1.05 | 320 to 400 ms | bezier (0.4, 0, 0.2, 1) | 2 to 5 percent | yes |
| camera | 100 to 480 px | 800 to 1600 ms | linear or (0.4, 0, 0.2, 1) | no overshoot, parallax 2 to 6 px | yes |

At mu 0.5 and gravity 1400 a slide of 100 px is 374 px/s over 535 ms, and the last 20 percent of the distance takes 45 percent of the time. A slide duration is rounded to a whole frame, so 535 ms at 12 fps becomes 580 ms and the last sample can sit a fraction short of the distance. The drop row carries no squash, because the field is refused, and the route refuses ballistic motion on a transform, so a falling element is keyed there. A fall time of sqrt(2h/g) often lands between frames: 100 px at g 1400 is 378 ms, so choose g or the travel to put the contact on a frame.

## 10.8 Failure modes and tests

| symptom | cause | fix |
|---|---|---|
| floaty | too long, gravity too low, no contact | cut 30 percent, gravity 1400, one contact frame |
| ice skating | constant rate, no plant | hold the contact, decelerate with a bezier |
| jitter | two layers on one property, or a spring near rest | one owner per property, rest delta 0.5 px |
| never settles | damping below 0.3, no settle cap | damping 0.5 or above, three overshoots maximum |
| contradicts mass | elastic on heavy, slow ease-in on light | z 0.8 to 1.0 heavy, 0.4 to 0.6 light |
| too smooth | no holds, everything overlapping | one or two frame holds, 2 to 3 px per frame |
| rest not read | a single frame of rest at the loop point | settle 3 frame times before the seam |
| bounce like a sine | equal heights or equal intervals | heights by e squared, intervals by e |
| pixel crawl | rounding every frame | snap at render only |
| loop seam | a spring or chain still moving | settle before the end |

Tests that pin the behaviour:

- A ballistic drop from 100 px at gravity 1400 reaches the plane within one frame of `sqrt(2h/g)`, and the plan carries a contact hold after each landing.
- Bounce heights decay by `e^2` within 5 percent, and the intervals by `e`.
- A settle reaches rest delta within the stated settle time, and never exceeds three visible overshoots.
- A rest-to-rest 100 px move over 10 frames with `cubic-bezier(0.4, 0, 0.2, 1)` has first and last deltas at most 30 percent of the largest; a move that ends at speed is judged at the seam instead.
- A chain with damping 0.35 or above comes to rest inside the plan, and never rings at the loop point.
- Overshoot measured from the evaluated curve matches the requested percentage, and a 100 px move with about 5.3 percent overshoot samples near 105.3 px at its peak; the 6 px cap is not implemented yet.

## 10.9 Surfaces

Sand and water are not rigid, and at 480x270 with 100 px per metre every grain and droplet is sub-pixel: a sand grain is 0.01 to 0.05 px and a raindrop 0.2 to 0.5 px. The viewer reads the bulk: the slope, the stream, the surface line and the ring. The timeline has no particle system and no simulation step, so a pour, a splash or a wave is a drawn shape with staggered tracks.

- Sand. A pour is a 2 to 4 px ribbon that tapers 20 to 40 percent, textured with 24 to 40 dots in three size classes, 1, 1.5 and 2 px, distributed 60/30/10. A pile is a triangle whose slope is 2:3 rise to run when dry, 1:1 when damp and 1:2 when saturated, so the dry base is three times the height; at a constant feed its height grows as sqrt(t). An avalanche starts 2 to 3 degrees above repose, moves the face 2 to 4 px over 2 to 3 frames, then holds the new slope. A pour is monotone, so it is played once or starts from a seeded pile; it cannot loop by itself.
- Water. A splash runs crown, jet, droplets, rings in that order. The crown is one held pose at 25 fps, 3 to 5 drop diameters wide, and cannot exist at 12 fps; the jet is 1 to 3 diameters tall and held 1 to 2 frames; 5 to 20 droplets leave staggered by 1 to 2 frames; two or three rings, 1 px thick and 1 to 2 px of amplitude, are staggered 3 to 4 frames and fade as r^-0.5. A wave band is three or four strips with their own phase, not one sine on the whole band.
- Copies. A stream, spray or foam is the main element plus two or three extra copies at 40 to 60 percent opacity, spaced by the per-frame travel, and one copy at 12 fps. Stagger them 40, 80 or 120 ms at 25 fps, or 80, 170 or 250 ms at 12 fps. The timeline has no copy field, so each copy is its own track and element.
- Palette. Material colours must be exact palette entries. Error diffusion on a 1 px dot moves the halo, so dither only areas over 4 px. The palette pass keeps at most 12 samples, so a crown or dent that lasts 1 to 2 frames can miss them; its extreme pose is added to the picks. APNG is used when colour matters.

## 10.10 Deformation

A deformation faster than the frame step is a pose swap, not a mesh. A batting net takes a 25 to 40 px dent in 1.4 to 3 frames at 25 fps and rebounds once, so three to five drawn poses on opacity holds read as a simulation, with the first pose already at full depth and already flat along the contact normal (10.3); the approach is not drawn. A 7.5 px ball at 40 m/s moves 160 px per frame and never reads as a dot: the shot starts near contact and the ball is a streak.

A slow deformation, such as a flag, cloth or jelly, is a lattice: one track of vertex offsets, projective cells, and `matrix3d` on a clipped copy of the art per cell. Four corners give the homography, which keeps shared edges straight; affine cells and two triangles per quad kink at the diagonal and are not the primitive. A 16x10 grid holds 0.5 to 1 px of error over a 300x200 panel. Where a part meets a surface, the vertices on the surface stay flat along the normal while the rest deforms, which is the contact face from 10.3. The tween route has no `mesh` kind and refuses path kinds, so this is drawn as poses or rendered frames until the kind exists. A mesh export would be baked and would not round trip through import.
