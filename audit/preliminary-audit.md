# Preliminary audit

Findings from the doc-to-code pass that are code against a binding rule, not stale text. Each item needs a decision before the audit round changes either the code or the spec.

## 1. Pingpong has no effect on the tween route

Rule: 11.2.1 makes `timing.loop` the time map of 04.5, and 12.3 says the sampled plan already carries the mirrored pingpong values and the repeat is the same restart the GIF does.

Code: `write_tween` builds `count` from `duration_us` alone and calls `FramePlan::uniform` (`rust/crates/anikit-render-svg/src/timeline.rs:61`). `Timing::map` mirrors only when the mapped time exceeds the duration (`rust/crates/anikit-tween/src/time.rs:54`), and no presentation time does. The export writes samples over the duration and declares `infinite` for cycle and pingpong (`rust/crates/anikit-render-svg/src/export.rs:85`).

Effect: a pingpong scene renders like a once scene in GIF and APNG, and the SVG restarts from the first key instead of playing back. A scene whose last key differs from its first jumps at the seam.

Decision: double the plan for pingpong, reject pingpong in the tween route until the plan carries it, or write `alternate` in the SVG export.

## 2. Spring settle cap not implemented

Rule: 10.2 says the explicit settle time is capped at 1.5 times the visual duration.

Code: `SpringPlan::prepare` rejects a zero settle and a settle longer than the segment (`rust/crates/anikit-tween/src/ease.rs:351` and `:357`). No 1.5 factor exists.

Effect: a settle longer than its segment fails the prepare, and the soft cap never applies.

Decision: implement the cap, or change 10.2 to the prepare rule the code enforces.

## 3. Six pixel overshoot cap not implemented

Rule: 10.5 says the renderer caps overshoot at 6 px regardless of travel, and the skill reference repeats it.

Code: no cap exists. The tween crate evaluates the curve and the renderer writes the sampled transform; neither crate holds a 6 px limit.

Effect: an overshooting bezier or spring can move an element further than 6 px, or off the canvas.

Decision: implement the cap in the tween crate or the renderer, or demote it to an authoring rule in 10 and the skill reference.

## 4. FramePlan::without_last has no caller

Code: the method and its test exist (`rust/crates/anikit-time/src/lib.rs:103` and `:187`), and no route calls it. 04.2 now says the tween plan has no duplicate to drop.

Effect: none on output. Decision: keep it as public API for a future route, or remove it with the test.
