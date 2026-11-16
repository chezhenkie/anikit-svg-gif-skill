# 00-overview.md - animated-gif skill

## Header

- Title: animated-gif skill - animation output from text frames, image folders, SVG pages and a JSON tween timeline, frames back out of a GIF or APNG, and an animated SVG back into a timeline, in one Rust binary.
- Purpose: the reference for the skill and its binary: the six routes, the shared frame writer for GIF and APNG, the SVG export and import, the frame flow, and the tween crate that carries keyframes and layers. Specsheet and code together describe the system.
- Stakeholders: the person and the agent invoking the skill, users of the public repository, and the runtime the vector and tween routes need: an installed Chromium based browser.
- Status: one Rust binary ships six routes and three animation output formats. The Python payload is deleted; it stays at tag python-freeze as a reference for behaviour, not an authority. The Rust workspace carries core, time, all four render routes and the tween crate; the tween crate implements parts 04 to 07 for number, opacity, transform and colour, and carries the path, raster and grid plans of part 06. The tween route is the crate's first consumer and renders an SVG drawing from the canonical JSON timeline; it also exports that timeline as an animated SVG. The extract route runs the other way: it reads a GIF or APNG back into numbered PNG frames and a timing manifest, and the images route rebuilds the timeline from that manifest. The import route closes the loop from an animated SVG to the canonical timeline, so the vector route can feed the tween engine.
- Related documents: README.md (install and invocation), CHANGELOG.md (change list), docs/index.md (human manual), docs/research/ (evidence and analysis), LICENSE.
- Evidence pin: behaviour claims cite the frozen payload at tag python-freeze (files under agent-skill/bin) and the Rust sources under rust/; measurements cite docs/research/. Cites name files, not commits.

## Short description

The skill is an animation maker with a keyframe engine at its centre. Four kinds of input - text frames, an image folder, an SVG or HTML page, a static drawing with a JSON timeline - run through one pipeline: delays on the 10 ms grid, identical frames merged, the file written under a temp name and verified before it replaces the target. GIF and APNG are written from frames; the tween route also exports an animated SVG. The JSON timeline and the tween crate are the part the other directions build on.

The Rust binary keeps the commands and the visible behaviour, streams frames instead of holding them, and adds a tween crate so keyframes, easing and layered animation can be evaluated without a browser. The tween route is the first consumer: it evaluates the timeline and draws the result into an SVG page, or writes the same samples as CSS keyframes. An extract route reads a GIF or APNG back into frames and their presentation delays, and the images route rebuilds an animation from that folder and manifest. An import route reads an animated SVG through the browser and writes the same canonical timeline, so an existing CSS or SMIL animation can be edited as keyframes. Multi-format conversion between animated GIF, APNG, SVG animation and image folders follows the tween crate; it is not specified here beyond the rules it must obey.

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
| [11-tween-route.md](11-tween-route.md) | the tween route: JSON grammar, element state, frame count, errors and tests |
| [12-output-formats.md](12-output-formats.md) | output choice by extension, the APNG writer, the SVG export, errors and tests |
| [13-extract-and-rebuild.md](13-extract-and-rebuild.md) | the extract route, the timing manifest, and rebuilding the timeline through the images route |
| [14-import.md](14-import.md) | the import route: sampling an animated SVG into the canonical timeline |

## Design rules

1. GIF uses one palette per loop, sampled from frames spread across it. A palette per frame is rejected because colours flicker between frames. APNG is truecolor and builds no palette.
2. Delays are multiples of 10 ms and at least 20 ms, rounded on cumulative time so the error does not accumulate around the loop.
3. Identical consecutive frames merge into one frame with the summed delay, in GIF and APNG alike.
4. Every output is written under a temp name, read back, checked for frame count, duration and size, then renamed. A printed path is always a complete file; an exported SVG is compared byte for byte before the rename.
5. No ffmpeg. The video route was removed and stays removed.
6. Frames stream. The animation is never held as a list of finished frames.
7. Tween is a crate of its own. Renderers may depend on it, it depends on no renderer, and no renderer depends on another.
8. Time in the tween crate is integer microseconds, converted once. Floats appear only while preparing tables and weights.
9. The frozen Python is a reference for behaviour. Once a route is specified here, its Rust tests define correctness and the Python file is deleted.
10. ASCII only, LF endings, no dates in documents, and the version lives only in agent-skill/SKILL.md.
11. A GIF or APNG can be read back into frames and timing. Extraction writes one PNG per stored frame and a manifest beside it; the images route reads that manifest and rebuilds the timeline without a frame grid or a transition.
12. An animated SVG can be read into the canonical timeline. Import samples the five properties at the frame times the tween route will use and records them with linear segments, so a source animation can be edited as keyframes.

## Document rules

Specsheets in this folder are binding: they state what the system does and what the code must satisfy. README.md and docs/index.md are written for humans and stay short. docs/research/ holds evidence, measurements and prior art: it is never binding, and a specsheet wins where the two disagree.

Part and section numbers are stable, so a citation such as section 3.2 keeps resolving after later edits. A rule that is still open appears in 09 and is cited by row number from the part it affects.
