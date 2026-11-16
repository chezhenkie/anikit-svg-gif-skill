# Binary

This folder holds the `anikit` binary for the host, copied from a release asset:

| Platform | Asset | Name here |
|---|---|---|
| Linux x86_64, glibc or musl | `anikit-linux-x86_64` | `anikit` |
| Linux arm, musl | `anikit-linux-arm` | `anikit` |
| Windows x86_64 | `anikit-windows-x86_64.exe` | `anikit.exe` |
| Windows arm | `anikit-windows-arm.exe` | `anikit.exe` |
| macOS arm | `anikit-macos-arm` | `anikit` |

A fresh clone has no binary in this folder. If there is none for the host, the platform is unsupported: the skill does not build or download one while making an animation.

```sh
gh release download v0.6.0 --repo chezhenkie/anikit-svg-gif-skill --pattern anikit-linux-x86_64 --output agent-skill/bin/anikit
chmod +x agent-skill/bin/anikit
```
