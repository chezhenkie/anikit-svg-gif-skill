# 00-overview.md - animated-gif skill

## Header

- Title: animated-gif skill - GIF building from text frames, image folders and SVG animation in one Rust binary.
- Purpose: the reference for the skill and its binary: the three routes, the shared GIF core, the frame flow, and the tween crate that carries keyframes and layers. Specsheet and code together describe the system.
- Stakeholders: the person and the agent invoking the skill, users of the public repository, and the runtime the vector route needs: an installed Chromium based browser.
- Status: 0.2.0 ships one Rust binary, installed by the skill from a release asset. The Python payload is deleted; it stays at tag python-freeze as a reference for behaviour, not an authority. The Rust workspace carries core, time, all three routes and the tween crate; the tween crate implements parts 04 to 07 for number, opacity, transform and colour, and carries the path, raster and grid plans of part 06.
- Related documents: README.md (install and invocation), CHANGELOG.md (change list), docs/index.md (human manual), docs/research/ (evidence and analysis), LICENSE.
- Evidence pin: behaviour claims cite the frozen payload at tag python-freeze (files under agent-skill/bin) and the Rust sources under rust/; measurements cite docs/research/. Cites name files, not commits.

## Short description

The skill turns three kinds of input into one looping GIF: text frames drawn as monospace glyphs, a folder of images with sequence, crossfade or Ken Burns motion, and an SVG or HTML page captured through a browser at explicit times. Every route ends in the same core: one palette sampled across the loop, delays on the 10 ms grid, identical frames merged, the file written under a temp name and verified before it replaces the target.

The Rust binary keeps the commands and the visible behaviour, streams frames instead of holding them, and adds a tween crate so keyframes, easing and layered animation can be evaluated without a browser. Multi-format conversion between animated GIF, SVG animation and image folders follows the tween crate; it is not specified here beyond the rules it must obey.

## Table of contents

| part | covers |
|---|---|
| [01-behaviour-baseline.md](01-behaviour-baseline.md) | what the frozen Python does: routes, flags, GIF core rules, error contract, and what the port must match |
| [02-crate-split.md](02-crate-split.md) | the Rust crates, the dependency direction, and what each boundary owns |
| [03-frame-flow.md](03-frame-flow.md) | streaming frames against a palette pass, the sink contract, disposal, memory budget |
| [04-tween-model.md](04-tween-model.md) | time, tracks, keys, easing ownership, duplicates, loop modes, canonical input |
| [05-tween-evaluation.md](05-tween-evaluation.md) | bezier solving, springs, splines, precision and the cost of one sample |
| [06-tween-properties.md](06-tween-properties.md) | property types, prepare against sample, paths, raster, ASCII grids |
| [07-tween-composition.md](07-tween-composition.md) | layers, weights, composite operations, per-type add rules, masks |
| [08-fixtures-and-gate.md](08-fixtures-and-gate.md) | fixture export, structural checks, the gate, and the order Python files are deleted |
| [09-decisions.md](09-decisions.md) | one row per open question, with the lean and the status |
| [10-kinetic-motion.md](10-kinetic-motion.md) | motion that reads as physical: contact holds, weight cues, bounce decay, springs against beziers, defaults |

## Design rules

1. One palette per loop, sampled from frames spread across it. A palette per frame is rejected because colours flicker between frames.
2. Delays are multiples of 10 ms and at least 20 ms, rounded on cumulative time so the error does not accumulate around the loop.
3. Identical consecutive frames merge into one frame with the summed delay.
4. A GIF is written under a temp name, read back, checked for frame count, duration and size, then renamed. A printed path is always a complete file.
5. No ffmpeg. The video route was removed and stays removed.
6. Frames stream. The animation is never held as a list of finished frames.
7. Tween is a crate of its own. Renderers may depend on it, it depends on no renderer, and no renderer depends on another.
8. Time in the tween crate is integer microseconds, converted once. Floats appear only while preparing tables and weights.
9. The frozen Python is a reference for behaviour. Once a route is specified here, its Rust tests define correctness and the Python file is deleted.
10. ASCII only, LF endings, no dates in documents, and the version lives only in agent-skill/SKILL.md.

## Document rules

Specsheets in this folder are binding: they state what the system does and what the code must satisfy. README.md and docs/index.md are written for humans and stay short. docs/research/ holds evidence, measurements and prior art: it is never binding, and a specsheet wins where the two disagree.

Part and section numbers are stable, so a citation such as section 3.2 keeps resolving after later edits. A rule that is still open appears in 09 and is cited by row number from the part it affects.
