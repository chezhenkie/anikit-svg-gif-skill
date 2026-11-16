# 08-fixtures-and-gate

Status: all four routes have fixtures. The Rust gate is the only gate and covers the workspace: core, time, all four routes, the two frame writers and the SVG export, and the browser routes where a browser is installed.

## 8.1 Fixtures

The frozen Python exported one fixture set per route before its file was deleted. Each set holds the smallest input that exercises the route and the GIF it produced. The tween route did not exist before the port, so its reference is the first Rust run and its manifest is the record.

| route | fixture input | what the manifest records |
|---|---|---|
| text | a three-frame file with a per-frame delay and one SGR colour | frame count, per-frame delay, size, palette size |
| images | three images at different sizes, one with EXIF rotation | frame count, total duration, size, mode, fit |
| vector | an SVG with a CSS animation and one SMIL animation | frame count, total duration, size, preset, browser name |
| tween | a 120x80 drawing with three base tracks and a windowed add layer | frame count, per-frame delay, size, track count, browser name |

The manifest is a plain text file beside the fixture, one `key=value` per line, plus a line per frame with its delay and a hash of its pixels. Hashes are of the composited canvas, not of the file or of one patch: the encoder writes deltas, so a frame's picture only exists after every frame up to it has been drawn.

The tween fixture is also exported to `.svg` and fed back through the vector route: the export is checked for its keyframes and its lack of script, and the playback must move the box across the captured frames. APNG output is checked against the same recorded structure where a route already has a fixture, and by the png crate's decoder in `anikit-core/tests/apng.rs`.

A fixture is compared structurally, never byte for byte against the Python output: frame count, delays, size, and per-frame pixel hashes. The Rust encoder is allowed to differ in compression as long as the decoded frames match.

## 8.2 Structural checks

Every route is checked for the same facts, in both languages:

- Frame count and the sum of delays equal what the input asked for.
- Canvas size equals the requested size, or the derived size where the height was omitted.
- The first frame differs from the middle frame when the input animates.
- A hold produces one frame with the summed delay.
- A failed run prints one line beginning with `FAIL`, exits 1, writes no output and leaves no temp file.
- The written file opens with an independent decoder.
- An APNG opens in the png crate with the same frame count, size and delay sum, and a corrupted chunk is refused.

## 8.3 The gate

The Python gate was `python tools/check.py --strict`: three routes, the error contract and repository hygiene, 5 of 5. It was deleted with the payload (8.4) and the Rust gate replaced it.

The porting checklist that gates each route merge is docs/research/python-to-rust-refactor-pitfalls.md. A route is not merged until that checklist passes for it, because the list records the behaviours that break silently in a port.

The Rust gate adds:

- `cargo test` for the workspace, including the fixture comparisons.
- `cargo clippy -- -D warnings` and `cargo fmt --check`.
- A musl build of the text and images routes, since those two must work with no system libraries (part 02).
- The hygiene checks ported from the Python gate: ASCII only, LF only, no secrets, no machine paths, and every markdown link resolving. They live in `rust/crates/anikit-cli/tests/repo_hygiene.rs`.
- A memory check: a 1000-frame run stays within the stated multiple of a 100-frame run (part 03). The test is `tests/memory.rs` in `anikit-core` and reads the peak resident set from the operating system.

The Rust gate runs in GitHub Actions on every push to `main` and every pull request (`.github/workflows/ci.yml`), on Linux x86_64, Linux arm, Windows x86_64, Windows arm and macOS arm. The arm jobs build the release binary; the Linux arm job runs the text and images routes from it, and the Windows arm job runs the text route to GIF and APNG and the tween route to SVG. A developer machine is a convenience, not the gate: a release asset is only ever produced by `.github/workflows/release.yml` on a tag (09, row 20).

## 8.4 Deletion order

1. A route's Python file is deleted in the same change that makes its Rust route pass the fixture set and the structural checks.
2. `gifkit.py` goes last, when no Python route remains.
3. `render_frames.cjs` and the Playwright dependency go with the vector route, since nothing else uses them.
4. `tools/check.py` is replaced by the Rust gate, not kept beside it.
5. The skill's install instructions, version and changelog change in the same release.

All five steps are done at 0.2.0.

The order in which the three routes are ported is decided in 09, row 12.
