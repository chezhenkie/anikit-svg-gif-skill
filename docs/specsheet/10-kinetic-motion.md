# 10-kinetic-motion

Status: implemented for ballistic and arc motion on scalar tracks; squash, stretch and friction are carried for the renderer. Evidence and sources: docs/research/kinetic-motion.md. This part turns that report into rules the crate and the routes must satisfy.

## 10.1 Why these rules

The research changed four things about how the tween crate is designed.

- Weight is carried by the edges of a move, not the middle. Gravity is the same for every mass, so a heavy object does not fall faster; it anticipates longer, holds contact longer, bounces less and overshoots less. A crate that only offers easing cannot express that, which is why a ballistic segment and a contact hold are needed.
- What a viewer reads is spacing, the gap between frames, not the curve name. Two curves with the same name can produce very different deltas, so the tests must assert delta profiles, not curve shapes.
- A spring is a reaction and a bezier is choreography. Springs are right for landings, settles and follow-through; beziers are right for entrances, exits, camera moves and anything that must close a loop. A spring's duration is emergent, which collides with a fixed frame plan, so its settle time has to be computed and capped.
- The frame budget is the real constraint. At 25 fps a two second loop is 50 frames, and one default spring at 1.07 s eats 27 of them. Defaults must be short, and a chain or a spring that is still moving at the loop point is a defect.

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
- `Arc` is a quadratic through a control point offset from the chord midpoint by `bend` times the chord length. A bend of 0.1 bows a 100 px move by 10 px.
- A follow-through chain is not a segment. It reads another track, so it is a layer binding with a delay, which is the additive path already defined in part 07.

Ballistic fields, all compiled at prepare:

| field | unit | meaning | default |
|---|---|---|---|
| `gravity` | px/s^2 | acceleration on the value axis, positive down | 1400 |
| `v0` | px/s | initial velocity, signed | 0 |
| `plane` | px | value at which contact occurs | required |
| `restitution` | 0 to 1 | speed ratio kept at contact | 0.5 |
| `friction` | 0 to 1 | horizontal loss per contact | 0.3 |
| `min_bounce` | px | stop when the rebound is below this | 1.0 |
| `max_speed` | px/s | terminal speed cap | 1000 |
| `contact_hold_us` | us | hold at the plane after contact | 40000 |
| `squash` | fraction | height lost at contact | 0.2 |
| `stretch` | fraction | length gained at peak speed | 0.15 |

A spring gains two fields, because a settle cannot be reproduced from stiffness and damping alone: `rest_delta` in pixels (0.5) and `rest_speed` in px/s (2). The explicit settle time stays, and it is capped at 1.5 times the visual duration.

## 10.3 Contact, bounce and settle

- Every contact exists as a frame and is held. One frame at 25 fps, two for a heavy object, one or two at 12 fps. A contact of zero frames reads as a bounce off nothing, and it is the commonest single error in a tweened bounce.
- Successive bounce heights fall by the square of restitution and the intervals fall by restitution. A height ratio between 0.35 and 0.55 reads natural. Above 0.7 it looks mechanical; below 0.2 it looks like sand.
- A GIF has no room for a full decay. A 100 px drop at restitution 0.55 runs 30 frames to rest, so cut to two bounces and a settle, or lower restitution to 0.4.
- Squash preserves area in two dimensions: width = 1 / height. A 20 percent squash widens 25 percent. Contact squash is 15 to 25 percent, stretch at speed 15 to 30 percent.
- The last pixels are the tell. The final 2 to 3 px take 2 to 3 frames at 0.5 to 1 px per frame. A move that goes 4 px then 0 reads as a snap.
- A slide decelerates with friction `mu` between 0.4 and 0.6 from 200 to 400 px/s, and the last 20 percent of the distance takes 40 percent of the time.

## 10.4 Weight

Weight is not speed. These are the cues that carry it, light against heavy:

| cue | light | heavy |
|---|---|---|
| anticipation | 2 to 3 frames | 6 to 10 frames |
| contact hold | 1 frame | 2 to 4 frames |
| bounces | 3 to 5 | 0 to 1 |
| overshoot | 8 to 12 percent | 2 percent |
| squash at contact | 20 to 30 percent | 8 to 12 percent |
| secondary motion | high and fast | low and slow |
| arc | drifting | short and straight |
| damping ratio | 0.4 to 0.6 | 0.8 to 1.0 |

Gravity is a scene setting, not a per-object one. A scene uses 1400 px/s^2 for a cartoon read, 981 px/s^2 for a plausible one, and holds one value throughout, because two objects falling at different rates in the same shot read as an error.

## 10.5 Springs and curves

- Damping ratio sets overshoot, natural frequency sets pace. 0.4 to 0.55 is playful, 0.6 to 0.75 is confident, 0.8 to 0.95 is crisp, 1.0 and above never overshoots. Frequency 8 to 12 rad/s is heavy, 15 to 25 is normal, 30 to 40 is snappy.
- Use a spring for a reaction: a landing, a settle, a follow-through, an interruption. Use a bezier for choreography: an entrance, an exit, a camera move, a loop.
- A bezier's duration is authored and a spring's is emergent, so a route with a fixed frame count prefers a bezier.
- Overshoot is not a control point value. Writing 1.05 into the first y handle produces 0.04 percent, not 5; y near 1.4 is what gives about 5.3 percent. The crate therefore measures overshoot from the evaluated curve, and the renderer caps it at 6 px regardless of travel.

## 10.6 Frame rate and small motion

- 12 fps reads as drawn because each pose is held long enough to see; 25 fps reads as smooth. 24 fps is not on the 10 ms grid and alternates 40 and 50 ms, so it is not a target.
- At 12 fps the delay alternates 80 and 90 ms, 83 on average, and the frame count closes the loop on the grid; use fewer links in a chain rather than longer delays.
- Positions stay sub-pixel through the tween and are snapped only at render, for text and strokes. Snapping inside the tween turns a 0.4 px per frame move into four 1 px jumps.
- A smear belongs at 25 fps, where it lasts 40 to 80 ms. At 12 fps it reads as a deliberate multiple.

## 10.7 Defaults

Frames are at 25 fps; at 12 fps prefer the long end of each range.

| case | travel | duration | segment | parameters | follow-through |
|---|---|---|---|---|---|
| entrance | 4 to 60 px | 240 to 320 ms | bezier (0, 0, 0.2, 1) | overshoot 2 to 4 percent | one link, 40 ms |
| exit | 4 to 60 px | 120 to 200 ms | bezier (0.4, 0, 1, 1) | none | none |
| drop | 40 to 200 px | 240 to 520 ms | ballistic | g 1400, e 0.5, two bounces | squash 20 percent |
| bounce | 8 to 40 px | 320 to 400 ms | spring | k 400, c 30, z 0.75 | one link, 40 ms |
| slide | 40 to 200 px | 320 to 480 ms | ease plus friction | v0 200 to 400 px/s, mu 0.5 | one frame tail |
| shake | 2 to 4 px | 120 to 200 ms | spring layer | 4 to 6 cycles, decay 0.5 | weight fade |
| pulse | scale 1.00 to 1.05 | 320 to 400 ms | bezier (0.4, 0, 0.2, 1) | 2 to 5 percent | none |
| camera | 100 to 480 px | 800 to 1600 ms | linear or (0.4, 0, 0.2, 1) | no overshoot | parallax 2 to 6 px |

## 10.8 Failure modes and tests

| symptom | cause | fix |
|---|---|---|
| floaty | too long, gravity too low, no contact | cut 30 percent, gravity 1400, one contact frame |
| ice skating | constant rate, no plant | hold the contact, decelerate, mu 0.4 to 0.6 |
| jitter | two layers on one property, or a spring near rest | one owner per property, rest delta 0.5 px |
| never settles | damping below 0.3, no settle cap | damping 0.5 or above, three overshoots maximum |
| contradicts mass | elastic on heavy, slow ease-in on light | z 0.8 to 1.0 heavy, 0.4 to 0.6 light |
| too smooth | no holds, everything overlapping | one or two frame holds, 2 to 3 px per frame |
| bounce like a sine | equal heights or equal intervals | heights by e squared, intervals by e |
| pixel crawl | rounding every frame | snap at render only |
| loop seam | a spring or chain still moving | settle before the end |

Tests that pin the behaviour:

- A ballistic drop from 100 px at gravity 1400 reaches the plane within one frame of `sqrt(2h/g)`, and the plan contains a held contact frame.
- Bounce heights decay by `e^2` within 5 percent, and the intervals by `e`.
- A settle reaches rest delta within the stated settle time, and never exceeds three visible overshoots.
- An eased 100 px move over 10 frames has first and last deltas at most 30 percent of the middle delta.
- A chain with damping 0.35 or above comes to rest inside the plan, and never rings at the loop point.
- Overshoot measured from the evaluated curve matches the requested percentage, and is capped at 6 px.
