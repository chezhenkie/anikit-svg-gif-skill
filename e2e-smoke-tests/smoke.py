#!/usr/bin/env python3
"""End-to-end smoke tests for an anikit binary.

The default run covers the routes that need no browser, so it works on a static
musl build or a release asset. --full adds the browser routes and the extract and
rebuild chain. Fixtures come from the Rust test tree, so the script can be run
from anywhere.

Usage:
    python e2e-smoke-tests/smoke.py path/to/anikit
    python e2e-smoke-tests/smoke.py path/to/anikit --full --out /tmp/smoke
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "rust" / "crates" / "anikit-cli" / "tests" / "fixtures"

FRAMES = re.compile(r"(\d+) frames, (\d+(?:\.\d+)?) s, (\d+) x (\d+)")
SAMPLES = re.compile(r"(\d+) samples, (\d+(?:\.\d+)?) s, (\d+) x (\d+)")
TRACKS = re.compile(r"(\d+) tracks, (\d+(?:\.\d+)?) s, (\d+) x (\d+)")

MAGIC = {
    ".gif": (b"GIF87a", b"GIF89a"),
    ".png": (b"\x89PNG\r\n\x1a\n",),
    ".apng": (b"\x89PNG\r\n\x1a\n",),
    ".svg": (b"<svg",),
    ".json": (b"{",),
}


class Report:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.skipped = 0

    def ok(self, name: str, detail: str) -> None:
        self.passed += 1
        print(f"PASS {name}: {detail}")

    def fail(self, name: str, detail: str) -> None:
        self.failed += 1
        print(f"FAIL {name}: {detail}")

    def skip(self, name: str, detail: str) -> None:
        self.skipped += 1
        print(f"SKIP {name}: {detail}")

    def summary(self) -> int:
        print(f"{self.passed} passed, {self.failed} failed, {self.skipped} skipped")
        return 1 if self.failed else 0


def run(binary: Path, args: list[str]) -> tuple[int, str]:
    proc = subprocess.run(
        [str(binary), *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    return proc.returncode, (proc.stdout + proc.stderr).strip()


def excerpt(text: str) -> str:
    line = next((item for item in text.splitlines() if item.strip()), "")
    return line[:120] if line else "no output"


def check_magic(path: Path) -> str:
    suffix = path.suffix.lower()
    head = path.read_bytes()[:8] if suffix != ".svg" else path.read_bytes()
    if suffix == ".svg":
        return "ok" if b"<svg" in head and b"@keyframes" in head else "missing svg or keyframes"
    if suffix == ".json":
        try:
            json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            return f"bad json: {error}"
        return "ok"
    accepted = MAGIC.get(suffix)
    if not accepted:
        return "ok"
    return "ok" if head.startswith(accepted) else f"bad {suffix} header {head!r}"


def parse(text: str, pattern: re.Pattern[str]) -> tuple[int, float, int, int] | None:
    match = pattern.search(text)
    if not match:
        return None
    return int(match.group(1)), float(match.group(2)), int(match.group(3)), int(match.group(4))


def case(
    report: Report,
    name: str,
    binary: Path,
    args: list[str],
    output: Path,
    pattern: re.Pattern[str],
    frames: int,
    size: tuple[int, int],
    unit: str = "frames",
) -> None:
    code, text = run(binary, args)
    if code != 0:
        report.fail(name, f"exit {code}: {excerpt(text)}")
        return
    found = parse(text, pattern)
    if not found:
        report.fail(name, f"no summary line: {excerpt(text)}")
        return
    count, _, width, height = found
    if count != frames:
        report.fail(name, f"{count} expected {frames}: {excerpt(text)}")
        return
    if (width, height) != size:
        report.fail(name, f"{width}x{height} expected {size[0]}x{size[1]}")
        return
    if not output.is_file() or output.stat().st_size == 0:
        report.fail(name, f"missing or empty {output.name}")
        return
    verdict = check_magic(output)
    if verdict != "ok":
        report.fail(name, verdict)
        return
    report.ok(name, f"{count} {unit}, {width}x{height}, {output.stat().st_size} bytes")


def main() -> int:
    parser = argparse.ArgumentParser(description="Route smoke tests for an anikit binary.")
    parser.add_argument("binary", type=Path, help="path to anikit or anikit.exe")
    parser.add_argument("--out", type=Path, help="output folder, default a fresh temp folder")
    parser.add_argument("--full", action="store_true", help="add the browser routes and the extract chain")
    options = parser.parse_args()

    binary = options.binary.resolve()
    if not binary.is_file():
        print(f"FAIL binary: not found at {binary}")
        return 1

    if options.out:
        out = options.out.resolve()
        out.mkdir(parents=True, exist_ok=True)
    else:
        out = Path(tempfile.mkdtemp(prefix="anikit-smoke-"))

    report = Report()
    text = FIXTURES / "text" / "frames.txt"
    images = FIXTURES / "images" / "input"
    vector = FIXTURES / "vector" / "input" / "animation.svg"
    scene = FIXTURES / "tween" / "input" / "scene.json"

    case(report, "ascii gif", binary, ["ascii", str(text), "-o", str(out / "ascii.gif"),
                                      "--size", "28", "--width", "480", "--height", "270"],
         out / "ascii.gif", FRAMES, 3, (480, 270))
    case(report, "ascii apng", binary, ["ascii", str(text), "-o", str(out / "ascii.apng"),
                                        "--size", "28", "--width", "480", "--height", "270"],
         out / "ascii.apng", FRAMES, 3, (480, 270))
    case(report, "images gif", binary, ["images", str(images), "-o", str(out / "images.gif"),
                                        "--width", "320", "--height", "180", "--mode", "crossfade",
                                        "--fps", "12", "--hold", "0.5", "--transition", "0.25"],
         out / "images.gif", FRAMES, 12, (320, 180))
    case(report, "tween svg", binary, ["tween", str(scene), "-o", str(out / "tween.svg")],
         out / "tween.svg", SAMPLES, 10, (120, 80), "samples")

    if options.full:
        case(report, "vector gif", binary, ["vector", str(vector), "-o", str(out / "vector.gif"),
                                            "--duration", "1", "--fps", "10"],
             out / "vector.gif", FRAMES, 10, (120, 80))
        case(report, "tween gif", binary, ["tween", str(scene), "-o", str(out / "tween.gif")],
             out / "tween.gif", FRAMES, 10, (120, 80))
        case(report, "tween apng", binary, ["tween", str(scene), "-o", str(out / "tween.apng")],
             out / "tween.apng", FRAMES, 10, (120, 80))

        source = out / "tween.gif"
        frames = out / "frames"
        if source.is_file():
            code, text = run(binary, ["extract", str(source), "-o", str(frames)])
            found = parse(text, FRAMES)
            if code != 0 or not found or found[0] != 10 or found[2:] != (120, 80):
                report.fail("extract gif", excerpt(text))
            elif not (frames / "manifest.json").is_file():
                report.fail("extract gif", "missing manifest.json")
            else:
                manifest = json.loads((frames / "manifest.json").read_text(encoding="utf-8"))
                delays = manifest.get("delays", [])
                missing = [
                    number
                    for number in range(1, len(delays) + 1)
                    if not (frames / f"frame-{number:04d}.png").is_file()
                ]
                if len(delays) != 10:
                    report.fail("extract gif", f"manifest holds {len(delays)} delays")
                elif (manifest.get("width"), manifest.get("height")) != (120, 80):
                    report.fail("extract gif", f"manifest size {manifest.get('width')}x{manifest.get('height')}")
                elif missing:
                    report.fail("extract gif", f"missing frame-{missing[0]:04d}.png")
                else:
                    report.ok("extract gif", f"{len(delays)} frames, 120x80, manifest read")

                code, text = run(binary, ["images", str(frames), "-o", str(out / "rebuilt.gif"),
                                          "--timing", str(frames / "manifest.json")])
                case_found = parse(text, FRAMES)
                target = out / "rebuilt.gif"
                if code != 0 or not case_found or case_found[0] != 10 or case_found[2:] != (120, 80):
                    report.fail("images timing", excerpt(text))
                elif not target.is_file() or check_magic(target) != "ok":
                    report.fail("images timing", "missing or bad rebuilt.gif")
                else:
                    report.ok("images timing", "10 frames, 120x80, rebuilt from the manifest")
        else:
            report.skip("extract gif", "tween gif missing")
            report.skip("images timing", "tween gif missing")

        exported = out / "tween.svg"
        imported = out / "scene.json"
        if exported.is_file():
            code, text = run(binary, ["import", str(exported), "-o", str(imported), "--duration", "1"])
            found = parse(text, TRACKS)
            if code != 0 or not found or found[0] != 3 or found[2:] != (120, 80):
                report.fail("import svg", excerpt(text))
            elif check_magic(imported) != "ok":
                report.fail("import svg", "bad scene json")
            else:
                report.ok("import svg", "3 tracks, 1.00 s, 120x80")
                case(report, "import render", binary,
                     ["tween", str(imported), "-o", str(out / "import.png")],
                     out / "import.png", FRAMES, 25, (120, 80))
        else:
            report.skip("import svg", "tween svg missing")
            report.skip("import render", "tween svg missing")

    status = report.summary()
    if not options.out and status == 0:
        shutil.rmtree(out, ignore_errors=True)
    return status


if __name__ == "__main__":
    sys.exit(main())
