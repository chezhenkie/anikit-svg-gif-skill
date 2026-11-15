# 08-fixtures-and-gate

Status: the text and images routes have fixtures; the vector route does not. The Python gate still covers every route, and the Rust gate covers core, time and both ported routes.

## 8.1 Fixtures

The frozen Python exports one fixture set per route before its file is deleted. Each set holds the smallest input that exercises the route and the GIF it produced.

| route | fixture input | what the manifest records |
|---|---|---|
| text | a three-frame file with a per-frame delay and one SGR colour | frame count, per-frame delay, size, palette size |
| images | three images at different sizes, one with EXIF rotation | frame count, total duration, size, mode, fit |
| vector | an SVG with a CSS animation and one SMIL animation | frame count, total duration, size, preset, browser name |

The manifest is a plain text file beside the fixture, one `key=value` per line, plus a line per frame with its delay and a hash of its pixels. Hashes are of decoded RGB, not of the file, because the encoder may change bytes without changing the picture.

A fixture is compared structurally, never byte for byte against the Python output: frame count, delays, size, and per-frame pixel hashes. The Rust encoder is allowed to differ in compression as long as the decoded frames match.

## 8.2 Structural checks

Every route is checked for the same facts, in both languages:

- Frame count and the sum of delays equal what the input asked for.
- Canvas size equals the requested size, or the derived size where the height was omitted.
- The first frame differs from the middle frame when the input animates.
- A hold produces one frame with the summed delay.
- A failed run prints one line beginning with `FAIL`, exits 1, writes no output and leaves no temp file.
- The written file opens with an independent decoder.

## 8.3 The gate

The Python gate is `python tools/check.py --strict`: three routes plus the error contract plus repository hygiene. It runs before every commit that changes behaviour and must pass 5 of 5.

The porting checklist that gates each route merge is docs/research/python-to-rust-refactor-pitfalls.md. A route is not merged until that checklist passes for it, because the list records the behaviours that break silently in a port.

The Rust gate adds:

- `cargo test` for the workspace, including the fixture comparisons.
- `cargo clippy -- -D warnings` and `cargo fmt --check`.
- A musl build of the text and images routes, since those two must work with no system libraries (part 02).
- The hygiene checks ported from the Python gate: ASCII only, LF only, no secrets, no machine paths, and every README link resolving.
- A memory check: a 1000-frame run stays within the stated multiple of a 100-frame run (part 03).

## 8.4 Deletion order

1. A route's Python file is deleted in the same change that makes its Rust route pass the fixture set and the structural checks.
2. `gifkit.py` goes last, when no Python route remains.
3. `render_frames.cjs` and the Playwright dependency go with the vector route, since nothing else uses them.
4. `tools/check.py` is replaced by the Rust gate, not kept beside it.
5. The skill's install instructions, version and changelog change in the same release.

The order in which the three routes are ported is decided in 09, row 12.
