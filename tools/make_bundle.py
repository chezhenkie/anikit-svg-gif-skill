#!/usr/bin/env python3
"""Build the anikit skill bundle: SKILL.md, INSTALL.md, references and one binary."""

import argparse
import zipfile
from pathlib import Path


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True, help="built binary for this target")
    ap.add_argument("--asset", required=True, help="release asset name, anikit-<platform>[.exe]")
    ap.add_argument("--tag", required=True, help="release tag, vX.Y.Z")
    ap.add_argument("--out", default="dist", help="output directory")
    args = ap.parse_args()

    root = Path(__file__).resolve().parent.parent
    skill = root / "agent-skill"
    binary = Path(args.binary)
    if not binary.is_file():
        raise SystemExit("binary not found: " + str(binary))
    if not args.asset.startswith("anikit-"):
        raise SystemExit("asset name must start with anikit-: " + args.asset)

    slug = args.asset[len("anikit-"):]
    if slug.endswith(".exe"):
        slug = slug[:-4]
        name = "anikit.exe"
    else:
        name = "anikit"
    version = args.tag[1:] if args.tag.startswith("v") else args.tag

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    bundle = out_dir / f"anikit-skill-{version}-{slug}.zip"
    with zipfile.ZipFile(bundle, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(skill / "SKILL.md", "anikit/SKILL.md")
        z.write(skill / "INSTALL.md", "anikit/INSTALL.md")
        z.write(binary, "anikit/bin/" + name)
        for ref in sorted((skill / "references").iterdir()):
            if ref.is_file():
                z.write(ref, "anikit/references/" + ref.name)
    print(f"{bundle}  {bundle.stat().st_size} bytes")


if __name__ == "__main__":
    main()
