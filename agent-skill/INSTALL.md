# anikit skill

This folder is the complete skill: SKILL.md, this file, the references and one anikit binary in bin. The bundle name carries the platform.

## Install

Extract the anikit folder into the skills directory of your agent:

- OpenCode: `~/.config/opencode/skills/`
- Codex: `~/.agents/skills/`
- Pi: `~/.pi/agent/skills/`
- OMP: `~/.omp/agent/skills/`

The result must be `skills/anikit/SKILL.md` and `skills/anikit/bin/anikit` (`anikit.exe` on Windows). That is the whole install: no download, no build, no Python or Node. The folder name must match the `name` in `SKILL.md`.

## Requirements

- The platform named in the zip. Other platforms have their own bundle on the releases page.
- A Chromium based browser for the vector, tween and import routes. The browser is found on the host and is not bundled. The ascii and images routes, extract and the SVG export run without one.

## Keep the folder together

anikit is the program. SKILL.md and the references are the instructions around it. Moving the binary out of bin, or copying only the binary, leaves the program without its skill document.

To use it, ask the agent for an animation; the skill loads from this folder.
