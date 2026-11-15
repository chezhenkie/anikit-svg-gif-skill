# 09-decisions

Status: living document. One row per question. Status is `open`, `decided` or `parked`. Lean is a recommendation, not a decision. A decided row cites the part that carries the rule.

| # | Question | Lean | Status | Decision |
|---|---|---|---|---|
| 1 | Frame flow: materialised frames or a deterministic frame source? | Stream through a sink, sample twice | decided | Streaming sink with a cheap palette pass, a two-slot pipeline, and delays derived from presentation times. Part 03. |
| 2 | Composition: reject stacked animations or support them? | Support, because reuse is the point | decided | Layers with a weight track, an explicit order and per-type composite rules. Part 07. |
| 3 | Easing ownership: outgoing or incoming key? | Outgoing, WAAPI and Lottie convention | decided | Outgoing. SMIL is shifted by one on import. Part 04. |
| 4 | Duplicate key times: reject, or allow a hold? | Allow only when the earlier key holds | decided | Allowed only as a hold; lookup takes the last key at or before the time. Part 04. |
| 5 | Repetition: inside the timeline or outside it? | Outside, as a time map | decided | Loop mode and rate live in `Timing`; play count lives in the frame plan. Part 04. |
| 6 | Mesh warp for raster keys: in the first version? | No | decided | Out of the first version; crossfade and affine cover the GIF cases. Part 06. |
| 7 | Package and binary naming | `anikit-` package prefix, binary `anikit` | decided | Packages `anikit-core`, `anikit-time`, `anikit-tween`, `anikit-render-text`, `anikit-render-raster`, `anikit-render-svg`, `anikit-cli`. One binary, `anikit` (`anikit.exe` on Windows), built by `anikit-cli`. Part 02. |
| 8 | Cargo features for optional routes | None; the crate split is the split | open | Features would let one build drop the browser route, but the skill installs all three. |
| 9 | Animated GIF or WebP as an image source | Expand every frame, route timing wins | open | The frozen Python reads frame 1 only and says nothing. Expansion needs its own delays reconciled with `--hold` and `--fps`. |
| 10 | Alpha policy on the way into GIF | Matte onto `--bg`, no semi-transparent output | open | GIF has 1-bit alpha. A matte is predictable; a threshold is sharper for text. |
| 11 | Font asset for the text route | No bundled font; system discovery plus an override | open | Bundling fixes metrics across platforms at the cost of repository size and licence review. Tests need a fixture font either way. |
| 12 | Route order and Python deletion | Text, images, vector; delete each after its tests pass, core last | open | Not decided. The tag `python-freeze` marks the reference point either way. |
| 13 | When the tween crate is built | After the Rust workspace exists | decided | It depends on `time` and `core` types, so it cannot be built before them. Parts 04 to 07 are its specification. |
| 14 | Repository name and release tags | `anikit-svg-gif-skill`; lightweight `vX.Y.Z` tags, existing tags kept | decided | The GitHub repository was renamed from `animated-gif-skill`, which keeps redirects. Tags `v0.1.0`, `v0.1.1` and `python-freeze` remain. The installed skill folder stays `animated-gif` because the folder name must match `SKILL.md`. |
| 15 | Alpine and musl build | Static `x86_64-unknown-linux-musl` binary; distribution Chromium for the vector route | decided | The text and images routes build as one static binary that runs on Alpine, glibc distributions and scratch containers. The vector route cannot use the Playwright browser there, because it is built against glibc; it uses the distribution `chromium` package instead. Alpine ships no fonts, so the text route needs a font package or an explicit path (row 11). |
| 16 | Motion vocabulary for weight and follow-through | Ballistic and arc segments, chains as layers, rest thresholds on springs | decided | A ballistic segment owns an absolute value and compiles into flights and contact holds; an arc is a quadratic bend on an eased move; a chain is a layer binding with a delay, not a segment. Springs gain `rest_delta` and `rest_speed` fields. Smear and blur stay renderer work, and squash is a deform track rather than a segment. Part 10. |

## Notes

- Rows 1 to 7 and 13 to 15 are settled by the research in docs/research/ and by the direction already agreed for the port. They are recorded here so a later change has to reopen them explicitly.
- Row 9 changes behaviour, so it needs the message text and the fixture at the same time.
- Row 10 interacts with row 9: an animated source with alpha needs a policy for every frame, not only the first.
