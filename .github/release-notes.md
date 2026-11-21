# anikit

One binary with six routes: ascii, images, vector and tween render animation, extract turns a GIF or APNG into frames and a timing manifest, and import turns an animated SVG into the timeline. Output is GIF, APNG, or an SVG export from the tween route.

## What is in this release

Each platform has a skill bundle, `anikit-skill-<version>-<platform>.zip`, holding `SKILL.md`, `INSTALL.md`, the references and the binary. The same platforms also list the bare binary for the clone route.

## Install

Download the bundle and extract the `anikit` folder into the skills directory for your agent: `~/.config/opencode/skills` for OpenCode, `~/.agents/skills` for Codex, `~/.pi/agent/skills` for Pi, `~/.omp/agent/skills` for OMP. Restart the agent if the skill does not appear.

Or fetch the same zip:

```sh
gh release download --repo chezhenkie/anikit-svg-gif-skill --pattern "anikit-skill-*-linux-x86_64.zip"
unzip anikit-skill-*-linux-x86_64.zip
mv anikit ~/.config/opencode/skills/anikit
```

On Windows use the `windows-x86_64` pattern and `Expand-Archive`. The folder name must match the `name` in `SKILL.md`.

## Requirements

One binary per platform, no Python, no Node and no package manager. The vector and import routes, and the tween route when it writes GIF or APNG, need a Chromium based browser. The ascii and images routes, extract and the SVG export run without one.

## Changes

The change list for this tag is in [CHANGELOG.md](https://github.com/chezhenkie/anikit-svg-gif-skill/blob/main/CHANGELOG.md).

## License

MIT. `anikit --licences` prints the third-party notices; they are also embedded in the binary.
