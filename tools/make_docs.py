"""Generate the sample GIFs used by the README.

Creates fixtures under scratch/docs and writes the three demo GIFs into docs/.
Run from the repository root, after building the binary:

    python tools/make_docs.py

The binary is taken from ANIKIT, then agent-skill/bin, then a local cargo build.
Only the standard library is used, so no Python packages are needed.
"""
import os
import pathlib
import shutil
import struct
import subprocess
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
WORK = ROOT / "scratch" / "docs"
ESC = chr(27)
SAMPLES = ("demo-ascii.gif", "demo-pics.gif", "demo-svg.gif")


def binary():
    names = ("anikit.exe",) if os.name == "nt" else ("anikit",)
    candidates = [pathlib.Path(os.environ["ANIKIT"])] if os.environ.get("ANIKIT") else []
    for name in names:
        candidates.append(ROOT / "agent-skill" / "bin" / name)
        for profile in ("release", "debug"):
            candidates.append(ROOT / "rust" / "target" / profile / name)
    for path in candidates:
        if path.is_file():
            return path
    raise SystemExit("no anikit binary; build it with cargo build --release or set ANIKIT")


def run(*args):
    proc = subprocess.run([str(BIN)] + [str(a) for a in args],
                          capture_output=True, text=True, timeout=600)
    if proc.returncode != 0:
        raise SystemExit((proc.stderr or proc.stdout).strip()[-400:])
    print(proc.stdout.strip())


def write_png(path, width, height, inside, colour):
    """Write an RGB PNG: `colour` where `inside(x, y)` is true, white elsewhere."""
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw.extend((245, 245, 240) if inside(x, y) else colour)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                     + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def circle(x, y):
    return (x - 320) ** 2 + (y - 200) ** 2 <= 120 ** 2


def square(x, y):
    radius = 24
    if not (200 <= x <= 440 and 80 <= y <= 320):
        return False
    near_x = min(abs(x - 200), abs(x - 440)) < radius
    near_y = min(abs(y - 80), abs(y - 320)) < radius
    if not (near_x and near_y):
        return True
    cx = 200 + radius if x < 320 else 440 - radius
    cy = 80 + radius if y < 200 else 320 - radius
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2


def triangle(x, y):
    ax, ay, bx, by, cx, cy = 320, 70, 455, 330, 185, 330
    d1 = (x - bx) * (ay - by) - (ax - bx) * (y - by)
    d2 = (x - cx) * (by - cy) - (bx - cx) * (y - cy)
    d3 = (x - ax) * (cy - ay) - (cx - ax) * (y - ay)
    return (d1 >= 0 and d2 >= 0 and d3 >= 0) or (d1 <= 0 and d2 <= 0 and d3 <= 0)


def ascii_sample():
    frames = []
    for pct in (0, 20, 40, 60, 80, 100):
        filled = round(pct / 100 * 16)
        bar = ESC + "[38;5;51m" + "#" * filled + ESC + "[38;5;240m" + "-" * (16 - filled)
        frame = [
            "  ",
            "  animated gif          ",
            "  [" + bar + ESC + "[0m] " + ESC + "[38;5;231m" + "%3d%%" % pct + ESC + "[0m  ",
            "  ascii route           ",
            "  ",
        ]
        frames.append("\n".join(frame))
    src = WORK / "loader.txt"
    src.write_text("\n---\n".join(frames) + "\n", encoding="ascii")
    run("ascii", src, "-o", DOCS / "demo-ascii.gif",
        "--size", 28, "--delay", 140, "--width", 480, "--height", 270,
        "--bg", "#0b0d12")


def pics_sample():
    folder = WORK / "pics"
    folder.mkdir(parents=True, exist_ok=True)
    specs = [((214, 92, 82), circle), ((64, 156, 128), square), ((86, 126, 214), triangle)]
    for i, (colour, shape) in enumerate(specs):
        write_png(folder / ("card-%d.png" % (i + 1)), 640, 400, shape, colour)
    run("images", folder, "-o", DOCS / "demo-pics.gif",
        "--mode", "crossfade", "--fps", 10, "--hold", 0.6,
        "--transition", 0.3, "--width", 480, "--height", 270)


def svg_sample():
    svg = WORK / "ring.svg"
    svg.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" '
        'viewBox="0 0 480 270">'
        '<rect width="480" height="270" fill="#0b0d12"/>'
        '<g id="ring">'
        '<circle cx="240" cy="72" r="18" fill="#ff7a66"/>'
        '<circle cx="302" cy="186" r="18" fill="#45c2b0"/>'
        '<circle cx="178" cy="186" r="18" fill="#7aa2ff"/>'
        '<circle cx="240" cy="150" r="7" fill="#e6edf6"/>'
        '</g></svg>', encoding="ascii")
    run("vector", svg, "-o", DOCS / "demo-svg.gif",
        "--animate", "rotate", "--selector", "#ring",
        "--duration", 2, "--fps", 12, "--background", "#0b0d12")


def main():
    global BIN
    BIN = binary()
    print("binary  %s" % BIN)
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    DOCS.mkdir(parents=True, exist_ok=True)
    ascii_sample()
    pics_sample()
    svg_sample()
    for name in SAMPLES:
        path = DOCS / name
        print("%s  %.1f KB" % (name, path.stat().st_size / 1024.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
