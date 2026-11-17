# Requirements

anikit runs from one binary per host. Three routes also need a Chromium based browser, and the ascii route embeds its own font. This page lists what each host and route needs and what the build needs; install commands and the asset names are in [README.md](README.md), and [docs/specsheet/](docs/specsheet/00-overview.md) holds the binding behaviour rules.

## Host platforms

| Host | Asset | Notes |
|---|---|---|
| Linux x86_64, glibc | `anikit-linux-x86_64` | static musl build; runs on glibc and musl systems |
| Linux arm, musl | `anikit-linux-arm` | static build |
| Windows x86_64 | `anikit-windows-x86_64.exe` | |
| Windows arm | `anikit-windows-arm.exe` | |
| macOS arm | `anikit-macos-arm` | Apple silicon |

A host without a matching asset is unsupported: the skill reports it and does not build or download a binary. There is no 32-bit asset and no Intel mac asset.

## Per route

| Route | Input | Browser | Other |
|---|---|---|---|
| `ascii` | text frames with optional ANSI colours | no | none |
| `images` | folder of images | no | image files the decoder reads |
| `images --timing` | folder of PNG frames and `manifest.json` | no | none |
| `vector` | SVG or HTML page | yes | local files only |
| `tween` to GIF or APNG | SVG drawing and JSON timeline | yes | none |
| `tween` to SVG | SVG drawing and JSON timeline | no | none |
| `import` | animated SVG, CSS or SMIL | yes | none |
| `extract` | animated GIF or APNG | no | target folder must not exist |

Images are read from PNG, JPEG, GIF, WebP, BMP and TIFF, with EXIF orientation applied. Output is GIF, APNG, an animated SVG from the tween route, a PNG frame folder with a manifest from extract, and the JSON timeline from import. The output extension chooses GIF or APNG; the tween route chooses the SVG export from `.svg`.

## Browser

The vector route, the tween route when it writes GIF or APNG, and the import route start a Chromium based browser once per run. Chromium, Google Chrome, Brave and Edge are searched in their install locations, then on `PATH`; `--browser PATH` or `GIFKIT_BROWSER` names one explicitly, so any other Chromium based browser works too. The browser runs headless, so no display server is needed. It starts with its own sandbox, and only a process that exits before opening its debugging port is started again with `--no-sandbox`, so a host that restricts unprivileged user namespaces still runs the routes. The page has no access to local files beyond the ones it loads itself: a script cannot read a further file through `fetch` or `XMLHttpRequest`. Scripts inside the drawing do run and can reach the network, so the artwork is trusted input until a script policy exists. A writable temporary directory is needed for the browser profile, which is removed when the run ends or fails.

The route has been tested with Chromium 146, ungoogled-chromium 156 and the Chrome on the GitHub Actions runners. No minimum version is enforced.

## Fonts

The ascii route carries DejaVu Sans Mono 2.37 in the binary, so a host needs no installed font. `--font PATH` or `ANIM_GIF_FONT` replaces it; a named file that does not exist fails the run, and no system font is searched. Other routes draw no text and need no fonts.

## Nothing else

No Python, Node, Playwright, ffmpeg, ImageMagick, package manager or network access is needed at run time. The skill takes the binary from a GitHub release asset and never downloads or builds anything while making an animation. The browser of the three page routes is the only runtime requirement outside the binary.

## Build

The workspace builds with stable Rust and cargo. CI checks formatting on Linux x86_64, runs `cargo clippy -D warnings` and `cargo test --workspace` on Linux x86_64, Windows x86_64 and Windows arm, and builds and smoke-tests the release binary on every target, with the static-link check on the two musl jobs. Release assets are built on GitHub Actions from a version tag, one runner per target; building all five locally needs the matching hosts or cross toolchains.

Every crate is pure Rust and the shipped assets are static where the platform needs it, so no C compiler or system library is required on the build host or the target. The only dev-only tools are cargo, rustup and Python for the smoke script and the sample generator.

## Disk and memory

Output is written under a temporary name beside the target, read back and renamed, so the output directory must be writable and an existing file is replaced. Extract assembles `<folder>.tmp` beside the target folder and renames it; the target folder must not exist when the run starts.

Frames stream: the GIF path holds one frame, the APNG path one RGB frame and one compressed frame. The suite asserts under 2 MiB of growth across 1000 frames at 480x270.

## Limits

- Canvas sides are 8 to 8192 pixels; the tween and import fps is 1 to 50, and the import fps must put its delay on the 10 ms grid.
- GIF holds up to 256 colours and one palette per loop; APNG is truecolor and builds no palette.
- Delays sit on the 10 ms grid and are at least 20 ms; identical consecutive frames merge into one.
- Samples in this repository are 480x270.
