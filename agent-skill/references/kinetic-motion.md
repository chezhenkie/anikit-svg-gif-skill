# Kinetic motion

Mandatory before choosing fps, delay, easing or a preset. Motion that reads as physical comes from spacing, contact and weight, not from a curve name. Numbers assume 480x270 at 25 fps; at 12 fps use the long end of each range.

## Rules

1. Give every move a frame count as well as a duration. At 25 fps a frame is 40 ms; at 12 fps the delay alternates 80 and 90 ms.
2. Spacing is timing: the first and last gap between frames must be at most 30 percent of the middle gap. A 100 px move over 10 frames has a middle gap near 20 px and end gaps of 1 to 3 px.
3. Every contact is a held frame: one frame at 25 fps, two for a heavy object, one or two at 12 fps. A contact of zero frames reads as a bounce off nothing.
4. Bounce heights fall by restitution squared and the intervals fall by restitution. A height ratio of 0.35 to 0.55 reads natural. A GIF rarely has room, so cut to two bounces and a settle.
5. The last pixels sell the weight: the final 2 to 3 px take 2 to 3 frames, at 0.5 to 1 px per frame. Stop a spring at 0.5 px or 2 px/s, and never let it cross the loop point.
6. Squash preserves area: width = 1 / height. Contact squash 15 to 25 percent, stretch at speed 15 to 30 percent.
7. Weight is carried by the edges, not the middle. Heavy means 6 to 10 frames of anticipation against 2 to 3, a longer contact, 0 or 1 bounces against 3 to 5, and 2 percent overshoot against 8 to 12. Gravity is the same for all masses; a heavy object does not fall faster.
8. Overshoot is measured from the evaluated curve, not written into a control point: y1 1.05 gives 0.04 percent, y1 1.4 gives about 5.3 percent. The 6 px cap is planned but not implemented, so keep travel small.
9. Springs are for reactions (landing, settle, follow-through, interruption); beziers are for choreography (entrance, exit, camera, loop). Damping 0.4 to 0.55 is playful, 0.6 to 0.75 confident, 0.8 to 1.0 crisp, 1.0 never overshoots.
10. Follow-through is a delay: one to two frames per link at 25 fps, damping at or above 0.35, gain at or below 0.85. Fewer links at 12 fps, not longer delays.
11. Keep positions sub-pixel through the move and snap only at render. Snapping every frame turns a 0.4 px move into 1 px jumps.
12. Nothing may still be moving at the loop seam.

## Defaults

| case | travel | duration | motion | follow-through |
|---|---|---|---|---|
| entrance | 4 to 60 px | 240 to 320 ms | bezier (0, 0, 0.2, 1), overshoot 2 to 4 percent | one link, 40 ms |
| exit | 4 to 60 px | 120 to 200 ms | bezier (0.4, 0, 1, 1) | none |
| drop | 40 to 200 px | 240 to 520 ms | ballistic, gravity 1400, restitution 0.5, two bounces | squash 20 percent |
| bounce | 8 to 40 px | 320 to 400 ms | spring, k 400, c 30 | one link, 40 ms |
| slide | 40 to 200 px | 320 to 480 ms | decelerate plus friction, mu 0.5 | one frame tail |
| shake | 2 to 4 px | 120 to 200 ms | spring layer, 4 to 6 cycles, decay 0.5 | weight fade |
| pulse | scale 1.00 to 1.05 | 320 to 400 ms | bezier (0.4, 0, 0.2, 1) | none |
| camera | 100 to 480 px | 800 to 1600 ms | linear or (0.4, 0, 0.2, 1) | parallax 2 to 6 px |

## Before delivering

- Every move has a frame count, and the ends are slower than the middle.
- Every contact exists as a held frame.
- Bounce heights decay, and the last bounce is small enough to read as a stop.
- Nothing is still moving at the loop point.
- The frame budget holds: 2 s at 25 fps is 50 frames.

Full specification: docs/specsheet/10-kinetic-motion.md in the source repository.
