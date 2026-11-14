"""Generate the sample GIFs used by the README.

Creates fixtures under scratch/docs and writes docs/ascii.gif, docs/pics.gif
and docs/svg.gif. Run from the repository root:

    python tools/make_docs.py
"""
import pathlib
import shutil
import subprocess
import sys

from PIL import Image, ImageDraw

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "agent-skill" / "bin"
DOCS = ROOT / "docs"
WORK = ROOT / "scratch" / "docs"
PY = sys.executable
ESC = chr(27)


def run(script, *args):
    proc = subprocess.run([PY, str(BIN / script)] + [str(a) for a in args],
                          capture_output=True, text=True, timeout=600)
    if proc.returncode != 0:
        raise SystemExit((proc.stderr or proc.stdout).strip()[-400:])
    print(proc.stdout.strip())


def ascii_sample():
    lines = []
    for pct in (0, 20, 40, 60, 80, 100):
        filled = round(pct / 100 * 22)
        bar = ESC + "[38;5;51m" + "#" * filled + ESC + "[38;5;240m" + "-" * (22 - filled)
        lines.append("animated gif")
        lines.append("  [" + bar + ESC + "[0m] " + ESC + "[38;5;231m" + "%3d%%" % pct + ESC + "[0m")
        lines.append("ascii route")
    text = ""
    for i in range(0, len(lines), 3):
        if i:
            text += "---\n"
        text += "\n".join(lines[i:i + 3]) + "\n"
    src = WORK / "loader.txt"
    src.write_text(text, encoding="ascii")
    run("ascii_gif.py", src, "-o", DOCS / "ascii.gif",
        "--size", 22, "--delay", 140, "--bg", "#0b0d12")


def pics_sample():
    folder = WORK / "pics"
    folder.mkdir(parents=True, exist_ok=True)
    specs = [((214, 92, 82), "circle"), ((64, 156, 128), "square"),
             ((86, 126, 214), "triangle")]
    for i, (color, shape) in enumerate(specs):
        im = Image.new("RGB", (640, 400), color)
        d = ImageDraw.Draw(im)
        if shape == "circle":
            d.ellipse((200, 80, 440, 320), fill=(245, 245, 240))
        elif shape == "square":
            d.rounded_rectangle((200, 80, 440, 320), radius=24, fill=(245, 245, 240))
        else:
            d.polygon([(320, 70), (455, 330), (185, 330)], fill=(245, 245, 240))
        im.save(folder / ("card-%d.png" % (i + 1)))
    run("pics_gif.py", folder, "-o", DOCS / "pics.gif",
        "--mode", "crossfade", "--fps", 10, "--hold", 0.6,
        "--transition", 0.3, "--width", 460)


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
        '</g></svg>', encoding="utf-8")
    run("svg_gif.py", svg, "-o", DOCS / "svg.gif",
        "--animate", "rotate", "--selector", "#ring",
        "--duration", 2, "--fps", 12, "--background", "#0b0d12")


def main():
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    DOCS.mkdir(parents=True, exist_ok=True)
    ascii_sample()
    pics_sample()
    svg_sample()
    for name in ("ascii.gif", "pics.gif", "svg.gif"):
        path = DOCS / name
        print("%s  %.1f KB" % (name, path.stat().st_size / 1024.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
