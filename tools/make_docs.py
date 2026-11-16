"""Generate the sample animations used by the README.

Creates fixtures under scratch/docs and writes the four demo animations into
docs/, plus the keyframe sample as APNG and as an animated SVG, and four white
line kinetic studies on black. The image sample is a dune ridge drawn at three
times of day, the SVG sample is an authored pulse monitor, and the keyframe
sample is an orrery driven by a JSON timeline.
Run from the repository root, after building the binary:

    python tools/make_docs.py

The binary is taken from ANIKIT, then agent-skill/bin, then a local cargo build.
Only the standard library is used, so no Python packages are needed.
"""
import json
import math
import os
import pathlib
import random
import shutil
import struct
import subprocess
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
WORK = ROOT / "scratch" / "docs"
ESC = chr(27)
SAMPLES = (
    "demo-ascii.gif",
    "demo-pics.gif",
    "demo-svg.gif",
    "demo-tween.gif",
    "demo-tween.apng",
    "demo-tween.svg",
    "demo-kinetic-drop.gif",
    "demo-kinetic-cradle.gif",
    "demo-kinetic-leap.gif",
    "demo-kinetic-wave.gif",
)


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


STILL_W, STILL_H = 480, 270
HORIZON = 150.0


def write_rgb_png(path, width, height, pixels):
    """Write an RGB8 PNG from a tight row-major buffer."""
    raw = bytearray()
    stride = width * 3
    for y in range(height):
        raw.append(0)
        raw.extend(pixels[y * stride:(y + 1) * stride])

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
                     + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def mix(a, b, k):
    return (a[0] + (b[0] - a[0]) * k,
            a[1] + (b[1] - a[1]) * k,
            a[2] + (b[2] - a[2]) * k)


def ramp(stops, t):
    """Piecewise linear colour lookup over (position, colour) stops."""
    if t <= stops[0][0]:
        return stops[0][1]
    for i in range(1, len(stops)):
        position, colour = stops[i]
        if t <= position:
            p0, c0 = stops[i - 1]
            u = 0.0 if position == p0 else (t - p0) / (position - p0)
            return mix(c0, colour, u)
    return stops[-1][1]


def ridge_far(x):
    return HORIZON + 9.0 * math.sin(x / 70.0 + 0.6) + 4.0 * math.sin(x / 23.0 + 2.1)


def ridge_mid(x):
    return 172.0 + 14.0 * math.sin(x / 105.0 + 1.9) + 5.0 * math.sin(x / 41.0 + 0.4)


def ridge_near(x):
    return 214.0 + 10.0 * math.sin(x / 150.0 + 2.7) + 6.0 * math.sin(x / 61.0 + 1.1)


DAWN = {
    "sky": [(0.0, (16, 20, 48)), (0.30, (44, 34, 78)), (0.58, (112, 58, 92)),
            (0.80, (204, 104, 96)), (1.0, (255, 178, 122))],
    "sun": (255, 232, 186),
    "glow": (255, 150, 96),
    "sun_at": (286.0, 122.0, 20.0),
    "dunes": [((124, 66, 84), (96, 50, 72), (255, 176, 136)),
              ((78, 38, 60), (54, 26, 44), (206, 112, 108)),
              ((36, 17, 34), (20, 10, 22), (118, 54, 76))],
    "stars": 0.2,
    "stars_count": 24,
    "moon": False,
    "fire": False,
    "birds": True,
}

NOON = {
    "sky": [(0.0, (26, 64, 96)), (0.35, (62, 116, 138)), (0.70, (140, 170, 158)),
            (1.0, (248, 206, 150))],
    "sun": (255, 246, 214),
    "glow": (255, 214, 150),
    "sun_at": (206.0, 78.0, 15.0),
    "dunes": [((196, 142, 92), (168, 114, 74), (255, 214, 160)),
              ((160, 98, 62), (122, 70, 46), (240, 176, 116)),
              ((94, 54, 36), (58, 32, 24), (198, 128, 82))],
    "stars": 0.0,
    "stars_count": 0,
    "moon": False,
    "fire": False,
    "birds": False,
}

NIGHT = {
    "sky": [(0.0, (4, 8, 18)), (0.40, (8, 20, 38)), (0.75, (16, 42, 58)),
            (1.0, (28, 64, 78))],
    "sun": (232, 240, 255),
    "glow": (128, 176, 210),
    "sun_at": (374.0, 58.0, 17.0),
    "dunes": [((30, 48, 62), (22, 34, 48), (96, 146, 164)),
              ((16, 28, 40), (10, 18, 28), (66, 106, 124)),
              ((8, 13, 21), (4, 7, 12), (44, 74, 90))],
    "stars": 1.0,
    "stars_count": 90,
    "moon": True,
    "fire": True,
    "birds": False,
}


def dune_bytes(scene):
    """One 480x270 dune scene, drawn at 2x and box filtered down."""
    ss = 2
    w, h = STILL_W * ss, STILL_H * ss
    sky = [ramp(scene["sky"], min(1.0, ((y + 0.5) / ss) / HORIZON)) for y in range(h)]
    far = [ridge_far((x + 0.5) / ss) for x in range(w)]
    mid = [ridge_mid((x + 0.5) / ss) for x in range(w)]
    near = [ridge_near((x + 0.5) / ss) for x in range(w)]
    buf = bytearray(w * h * 3)

    def write(x, y, colour):
        i = (y * w + x) * 3
        buf[i] = int(colour[0])
        buf[i + 1] = int(colour[1])
        buf[i + 2] = int(colour[2])

    def blend(x, y, colour, alpha):
        if alpha <= 0.0:
            return
        i = (y * w + x) * 3
        k = min(1.0, alpha)
        for c in range(3):
            buf[i + c] = int(buf[i + c] + (colour[c] - buf[i + c]) * k)

    def disc(cx, cy, radius, colour, alpha=1.0):
        x0 = max(0, int((cx - radius - 1.5) * ss))
        x1 = min(w, int((cx + radius + 1.5) * ss) + 1)
        y0 = max(0, int((cy - radius - 1.5) * ss))
        y1 = min(h, int((cy + radius + 1.5) * ss) + 1)
        for yy in range(y0, y1):
            fy = (yy + 0.5) / ss
            for xx in range(x0, x1):
                fx = (xx + 0.5) / ss
                d = math.hypot(fx - cx, fy - cy)
                coverage = min(1.0, max(0.0, radius + 0.5 - d))
                blend(xx, yy, colour, coverage * alpha)

    def glow(cx, cy, radius, colour, strength):
        x0 = max(0, int((cx - radius) * ss))
        x1 = min(w, int((cx + radius) * ss) + 1)
        y0 = max(0, int((cy - radius) * ss))
        y1 = min(h, int((cy + radius) * ss) + 1)
        for yy in range(y0, y1):
            fy = (yy + 0.5) / ss
            for xx in range(x0, x1):
                fx = (xx + 0.5) / ss
                d = math.hypot(fx - cx, fy - cy) / radius
                if d < 1.0:
                    blend(xx, yy, colour, strength * (1.0 - d) ** 2.4)

    def stroke(x0, y0, x1, y1, width, colour, alpha=1.0):
        vx, vy = x1 - x0, y1 - y0
        length2 = vx * vx + vy * vy
        pad = width * 0.5 + 1.5
        bx0 = max(0, int((min(x0, x1) - pad) * ss))
        bx1 = min(w, int((max(x0, x1) + pad) * ss) + 1)
        by0 = max(0, int((min(y0, y1) - pad) * ss))
        by1 = min(h, int((max(y0, y1) + pad) * ss) + 1)
        for yy in range(by0, by1):
            py = (yy + 0.5) / ss
            for xx in range(bx0, bx1):
                px = (xx + 0.5) / ss
                u = 0.0
                if length2 > 0.0:
                    u = ((px - x0) * vx + (py - y0) * vy) / length2
                    u = min(1.0, max(0.0, u))
                d = math.hypot(px - (x0 + u * vx), py - (y0 + u * vy))
                coverage = min(1.0, max(0.0, width * 0.5 + 0.5 - d))
                blend(xx, yy, colour, coverage * alpha)

    # Sky, sun or moon, then the stars behind everything else.
    for y in range(h):
        base = sky[y]
        for x in range(w):
            write(x, y, base)

    sx, sy, sr = scene["sun_at"]
    glow(sx, sy, sr * 4.2, scene["glow"], 0.55)
    if scene["moon"]:
        # A crescent is the lit disc minus an offset shadow disc, painted with
        # a clean edge instead of a sky-coloured shadow that shows the halo.
        x0 = max(0, int((sx - sr - 2) * ss))
        x1 = min(w, int((sx + sr + 2) * ss) + 1)
        y0 = max(0, int((sy - sr - 2) * ss))
        y1 = min(h, int((sy + sr + 2) * ss) + 1)
        for yy in range(y0, y1):
            fy = (yy + 0.5) / ss
            for xx in range(x0, x1):
                fx = (xx + 0.5) / ss
                d = math.hypot(fx - sx, fy - sy)
                cover = min(1.0, max(0.0, sr + 0.5 - d))
                shadow = min(1.0, max(0.0, sr + 0.5 - math.hypot(fx - (sx - 7.0), fy - (sy - 5.0))))
                blend(xx, yy, scene["sun"], cover * (1.0 - shadow))
    else:
        disc(sx, sy, sr, scene["sun"])

    rng = random.Random(7)
    for _ in range(scene["stars_count"]):
        dx = rng.uniform(10.0, STILL_W - 10.0)
        dy = rng.uniform(8.0, HORIZON - 18.0)
        radius = rng.uniform(0.5, 1.2)
        bright = rng.uniform(0.35, 1.0) * scene["stars"]
        disc(dx, dy, radius, (240, 238, 220), bright)
        if radius > 1.05:
            glow(dx, dy, 4.0, (200, 220, 255), 0.25 * scene["stars"])

    # Three ridges share one silhouette; only the palette changes.
    layers = ((far, scene["dunes"][0], False), (mid, scene["dunes"][1], False),
              (near, scene["dunes"][2], True))
    for y in range(h):
        fy = (y + 0.5) / ss
        for x in range(w):
            fx = (x + 0.5) / ss
            colour = None
            for heights, (top, bottom, rim), ripples in layers:
                edge_line = heights[x]
                if fy < edge_line:
                    continue
                depth = (fy - edge_line) / max(1.0, STILL_H - edge_line)
                colour = mix(top, bottom, min(1.0, depth * 1.35))
                crest = max(0.0, 1.0 - (fy - edge_line) / 4.0)
                colour = mix(colour, rim, crest * 0.45)
                if ripples and 6.0 < fy - edge_line < 42.0:
                    for offset in (14.0, 26.0, 36.0):
                        line = edge_line + offset + 2.5 * math.sin(fx / 33.0 + offset * 0.2)
                        gap = abs(fy - line)
                        if gap < 1.1:
                            colour = mix(colour, rim, 0.10 * (1.0 - gap / 1.1))
            if colour is not None:
                write(x, y, colour)

    ground = ridge_mid(330.0)
    ink = (16, 8, 18) if scene["dunes"][0][0][0] < 120 else (30, 18, 22)
    stroke(330.0, ground - 13.0, 330.0, ground - 2.0, 3.4, ink)
    disc(330.0, ground - 15.6, 2.6, ink)
    stroke(330.0, ground - 2.0, 326.5, ground + 1.5, 1.4, ink)
    stroke(330.0, ground - 2.0, 333.5, ground + 1.5, 1.4, ink)
    stroke(332.0, ground - 12.0, 338.0, ground - 1.0, 1.2, ink)
    if scene["fire"]:
        fire = ridge_mid(352.0) + 1.0
        glow(352.0, fire - 2.0, 14.0, (255, 174, 92), 0.6)
        disc(352.0, fire - 1.0, 1.6, (255, 214, 150))
    if scene["birds"]:
        for bx, by in ((188.0, 92.0), (207.0, 84.0), (226.0, 96.0)):
            stroke(bx - 4.0, by, bx, by - 2.5, 1.1, ink, 0.55)
            stroke(bx, by - 2.5, bx + 4.0, by, 1.1, ink, 0.55)

    # Box filter the 2x buffer.
    out = bytearray(STILL_W * STILL_H * 3)
    for y in range(STILL_H):
        for x in range(STILL_W):
            i = (y * STILL_W + x) * 3
            total = [0.0, 0.0, 0.0]
            for dy in range(2):
                for dx in range(2):
                    j = ((y * 2 + dy) * w + x * 2 + dx) * 3
                    for c in range(3):
                        total[c] += buf[j + c]
            for c in range(3):
                out[i + c] = int(total[c] * 0.25 + 0.5)
    return bytes(out)


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
    for name, scene in (("dune-1-dawn", DAWN), ("dune-2-noon", NOON),
                        ("dune-3-night", NIGHT)):
        write_rgb_png(folder / (name + ".png"), STILL_W, STILL_H, dune_bytes(scene))
    run("images", folder, "-o", DOCS / "demo-pics.gif",
        "--mode", "crossfade", "--fps", 14, "--hold", 1.0,
        "--transition", 0.8, "--width", STILL_W, "--height", STILL_H)


def ecg_path(period=160, beats=8, start=-160, base=226):
    shape = [(0, 0), (28, 0), (36, -5), (46, -5), (54, 0), (64, 0),
             (70, 6), (76, -32), (82, 11), (90, 0), (104, 0),
             (112, -10), (126, -10), (138, 0), (160, 0)]
    parts = []
    for beat in range(beats):
        for index, (dx, dy) in enumerate(shape):
            x = start + beat * period + dx
            y = base + dy
            parts.append("%s%.0f %.0f" % ("M" if index == 0 and beat == 0 else "L", x, y))
    return " ".join(parts)


def svg_sample(fps=25, out_dir=DOCS):
    trace = ecg_path()
    style = """
#heart { transform-box: fill-box; transform-origin: center;
         animation: beat 800ms ease-in-out infinite; }
@keyframes beat {
  0%, 100% { transform: scale(1); }
  8% { transform: scale(1.10); }
  20% { transform: scale(0.97); }
  32% { transform: scale(1.05); }
  50% { transform: scale(1); }
}
.ring { fill: none; stroke: #ff5666; stroke-width: 1.4;
        transform-box: fill-box; transform-origin: center;
        animation: ring 800ms linear infinite; }
@keyframes ring {
  0% { transform: scale(0.55); opacity: 0; }
  15% { opacity: 0.65; }
  100% { transform: scale(1.55); opacity: 0; }
}
#trace { animation: scroll 800ms linear infinite; }
@keyframes scroll { from { transform: translateX(0); } to { transform: translateX(-160px); } }
.led { animation: led 800ms linear infinite; }
@keyframes led { 0%, 70%, 100% { opacity: 0.16; } 12%, 40% { opacity: 1; } }
"""
    svg_text = (
        '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" '
        'viewBox="0 0 480 270">'
        + "<style>" + style + "</style>"
        '<rect width="480" height="270" fill="#070c0b"/>'
        '<rect y="188" width="480" height="82" fill="#0a1411"/>'
        '<path d="M18 40V18H40 M440 18H462V40 M462 230V252H440 M40 252H18V230" '
        'stroke="#1e5144" stroke-width="2" fill="none" stroke-linecap="round"/>'
        '<circle cx="140" cy="134" r="100" fill="#ff3346" opacity="0.04"/>'
        '<circle cx="140" cy="134" r="82" fill="#ff3346" opacity="0.05"/>'
        '<circle cx="140" cy="134" r="64" fill="#ff3346" opacity="0.06"/>'
        '<circle class="ring" cx="140" cy="134" r="64"/>'
        '<g id="heart">'
        '<path d="M140 114 C134 102 118 94 104 100 C88 107 88 128 100 142 '
        'C112 156 128 168 140 182 C152 168 168 156 180 142 '
        'C192 128 192 107 176 100 C162 94 146 102 140 114 Z" '
        'fill="#ff3b50"/>'
        '<path d="M140 114 C134 102 118 94 104 100 C88 107 88 128 100 142 '
        'C112 156 128 168 140 182 C152 168 168 156 180 142 '
        'C192 128 192 107 176 100 C162 94 146 102 140 114 Z" '
        'fill="#ff8a94" opacity="0.22" '
        'transform="translate(138 130) scale(0.62) translate(-140 -134)"/>'
        '<path d="M140 114 C134 102 118 94 104 100 C88 107 88 128 100 142 '
        'C112 156 128 168 140 182 C152 168 168 156 180 142 '
        'C192 128 192 107 176 100 C162 94 146 102 140 114 Z" '
        'fill="none" stroke="#ff9aa2" stroke-width="1.1"/>'
        '<path d="M118 110 C108 108 101 116 99 125" stroke="#ffd0d4" '
        'stroke-width="2.6" stroke-linecap="round" fill="none" opacity="0.4"/>'
        "</g>"
        '<g id="trace">'
        '<path d="' + trace + '" stroke="#ff3346" stroke-width="7" '
        'stroke-linejoin="round" stroke-linecap="round" fill="none" '
        'opacity="0.16"/>'
        '<path d="' + trace + '" stroke="#ff5a6a" stroke-width="2.4" '
        'stroke-linejoin="round" stroke-linecap="round" fill="none"/>'
        "</g>"
        '<circle class="led" cx="432" cy="36" r="3.2" fill="#ff5666"/>'
        '<circle class="led" cx="448" cy="36" r="3.2" fill="#ff5666" '
        'style="animation-delay:-200ms"/>'
        '<circle class="led" cx="464" cy="36" r="3.2" fill="#ff5666" '
        'style="animation-delay:-400ms"/>'
        "</svg>"
    )
    svg = WORK / "pulse.svg"
    svg.write_text(svg_text, encoding="ascii")
    run("vector", svg, "-o", out_dir / "demo-svg.gif",
        "--duration", 1.6, "--fps", fps, "--background", "#070c0b")


def orbit_point(t, duration, rx, ry, tilt_deg, turns, phase, direction, cx=240.0, cy=138.0):
    angle = phase + direction * turns * 2.0 * math.pi * (t / float(duration))
    ox = rx * math.cos(angle)
    oy = ry * math.sin(angle)
    tilt = math.radians(tilt_deg)
    return (cx + ox * math.cos(tilt) - oy * math.sin(tilt),
            cy + ox * math.sin(tilt) + oy * math.cos(tilt))


def tween_sample(duration=3600, fps=20, out_dir=DOCS):
    step = 1000 // fps
    times = list(range(0, duration + 1, step))

    rng = random.Random(11)
    stars = []
    for _ in range(16):
        stars.append((rng.uniform(8.0, 472.0), rng.uniform(8.0, 262.0),
                      rng.uniform(0.7, 1.6), rng.uniform(0.2, 0.55)))
    twinkles = ((78.0, 42.0, 0.0), (404.0, 60.0, 2.1), (326.0, 214.0, 4.0))

    drawing = [
        '<ellipse cx="240" cy="138" rx="214" ry="106" fill="#0a111c"/>',
        '<ellipse cx="240" cy="138" rx="164" ry="82" fill="#0b1421"/>',
    ]
    for sx, sy, radius, bright in stars:
        drawing.append('<circle cx="%.1f" cy="%.1f" r="%.2f" fill="#dfe8ff" '
                       'opacity="%.2f"/>' % (sx, sy, radius, bright))
    for index, (sx, sy, phase) in enumerate(twinkles):
        drawing.append('<circle id="twinkle-%d" cx="%.1f" cy="%.1f" r="1.8" '
                       'fill="#fff4d8" opacity="0.9"/>' % (index, sx, sy))
    for sx, sy in ((128.0, 58.0), (424.0, 176.0), (58.0, 196.0)):
        drawing.append('<path d="M%.0f %.0fv8M%.0f %.0fh8" stroke="#dfe8ff" '
                       'stroke-width="1" opacity="0.5"/>'
                       % (sx, sy - 4.0, sx - 4.0, sy))
    drawing += [
        '<ellipse cx="240" cy="138" rx="72" ry="27" transform="rotate(-12 240 138)" '
        'fill="none" stroke="#33486a" stroke-width="1.3" stroke-dasharray="3 6"/>'
        '<ellipse cx="240" cy="138" rx="118" ry="45" transform="rotate(14 240 138)" '
        'fill="none" stroke="#33486a" stroke-width="1.3" stroke-dasharray="3 6"/>'
        '<ellipse cx="240" cy="138" rx="152" ry="58" transform="rotate(-20 240 138)" '
        'fill="none" stroke="#33486a" stroke-width="1.3" stroke-dasharray="3 6"/>'
        '<g transform="translate(240,138)">'
        '<circle r="58" fill="#ffb86b" opacity="0.04"/>'
        '<circle r="48" fill="#ffb86b" opacity="0.05"/>'
        '<circle r="38" fill="#ffb86b" opacity="0.07"/>'
        '<g id="sun">'
        '<circle r="18" fill="#ffe8b8"/>'
        '<circle r="25" fill="none" stroke="#f6b46a" stroke-width="0.9" opacity="0.4"/>'
        "</g></g>",
        '<g id="planet-a">'
        '<g id="spin-a">'
        '<circle r="8" fill="#ff6f59"/>'
        '<path d="M-6.4 -2 A7 7 0 0 1 5.2 4.3" fill="none" stroke="#ffc0ae" '
        'stroke-width="1.5" opacity="0.6"/>'
        "</g><g id=\"moon-a\"><circle cx=\"16\" r=\"2.4\" fill=\"#f3ead8\"/></g></g>",
        '<g id="planet-b">'
        '<g id="spin-b">'
        '<ellipse rx="23" ry="6.5" transform="rotate(-20)" fill="none" '
        'stroke="#8fe8d4" stroke-width="1.3" opacity="0.8"/>'
        '<circle r="11" fill="#3ecfb2"/>'
        '<path d="M-8 -6.5 A9.5 9.5 0 0 1 3 -10.2" fill="none" stroke="#9cf3df" '
        'stroke-width="1.8" opacity="0.55"/>'
        '<path d="M23 0 A23 6.5 0 0 1 -23 0" transform="rotate(-20)" fill="none" '
        'stroke="#8fe8d4" stroke-width="1.6"/>'
        "</g></g>",
        '<g id="planet-c"><circle r="5" fill="#ffc857"/></g>',
        '<g id="comet">'
        '<path d="M0 0 L-64 -3 L-50 0 L-64 3 Z" fill="#b98a4a" opacity="0.55"/>'
        '<path d="M0 0 L-32 -1.8 L-24 0 L-32 1.8 Z" fill="#ffe3b0" opacity="0.85"/>'
        '<circle cx="-16" r="1.1" fill="#ffe3b0" opacity="0.5"/>'
        '<circle cx="-26" r="0.9" fill="#ffe3b0" opacity="0.35"/>'
        '<circle r="2.4" fill="#fff6e0"/>'
        "</g>",
    ]

    tracks = []

    def transform_track(track_id, ident, fn):
        keys = []
        for t in times:
            key = {"t": t}
            key.update(fn(t))
            key["easing"] = "linear"
            keys.append(key)
        tracks.append({"id": track_id, "kind": "svg.transform",
                       "src": "orbit.svg#" + ident, "keys": keys})

    def position(rx, ry, tilt, turns, phase, direction):
        def fn(t):
            x, y = orbit_point(t, duration, rx, ry, tilt, turns, phase, direction)
            return {"translate": [round(x, 2), round(y, 2)]}
        return fn

    tracks.append({
        "id": "sun-scale", "kind": "svg.transform", "src": "orbit.svg#sun",
        "keys": [
            {"t": 0, "scale": [1.0, 1.0], "easing": "ease-in-out"},
            {"t": duration // 4, "scale": [1.045, 1.045], "easing": "ease-in-out"},
            {"t": duration // 2, "scale": [1.0, 1.0], "easing": "ease-in-out"},
            {"t": duration * 3 // 4, "scale": [1.04, 1.04], "easing": "ease-in-out"},
            {"t": duration, "scale": [1.0, 1.0]},
        ],
    })
    transform_track("planet-a-orbit", "planet-a", position(72.0, 27.0, -12.0, 2.0, 0.7, 1))
    transform_track("planet-b-orbit", "planet-b", position(118.0, 45.0, 14.0, 1.0, -0.6, -1))
    transform_track("planet-c-orbit", "planet-c", position(152.0, 58.0, -20.0, 2.0, 2.4, 1))
    tracks.append({"id": "planet-a-spin", "kind": "svg.transform",
                   "src": "orbit.svg#spin-a",
                   "keys": [{"t": 0, "rotate": 0.0, "easing": "linear"},
                            {"t": duration, "rotate": 360.0}]})
    tracks.append({"id": "planet-b-spin", "kind": "svg.transform",
                   "src": "orbit.svg#spin-b",
                   "keys": [{"t": 0, "rotate": 0.0, "easing": "linear"},
                            {"t": duration, "rotate": -180.0}]})
    tracks.append({"id": "moon-a-orbit", "kind": "svg.transform",
                   "src": "orbit.svg#moon-a",
                   "keys": [{"t": 0, "rotate": 0.0, "easing": "linear"},
                            {"t": duration, "rotate": 720.0}]})

    p0, pc, p1 = (-90.0, 24.0), (210.0, 20.0), (570.0, 186.0)

    def comet_state(t):
        u = t / float(duration)
        x = (1 - u) ** 2 * p0[0] + 2 * (1 - u) * u * pc[0] + u * u * p1[0]
        y = (1 - u) ** 2 * p0[1] + 2 * (1 - u) * u * pc[1] + u * u * p1[1]
        dx = 2 * (1 - u) * (pc[0] - p0[0]) + 2 * u * (p1[0] - pc[0])
        dy = 2 * (1 - u) * (pc[1] - p0[1]) + 2 * u * (p1[1] - pc[1])
        return x, y, math.degrees(math.atan2(dy, dx))

    def comet_motion(t):
        x, y, angle = comet_state(t)
        return {"translate": [round(x, 2), round(y, 2)], "rotate": round(angle, 2)}

    transform_track("comet-path", "comet", comet_motion)
    tracks.append({"id": "comet-glow", "kind": "svg.opacity", "src": "orbit.svg#comet",
                   "keys": [{"t": 0, "value": 0.0, "easing": "linear"},
                            {"t": duration * 8 // 100, "value": 0.9, "easing": "linear"},
                            {"t": duration * 9 // 10, "value": 0.9, "easing": "linear"},
                            {"t": duration, "value": 0.0}]})

    twinkle_period = duration / 2.0
    for index, (sx, sy, phase) in enumerate(twinkles):
        keys = []
        for i in range(31):
            t = round(duration * i / 30.0)
            wave = 0.5 + 0.5 * math.sin(2.0 * math.pi * t / twinkle_period + phase)
            keys.append({"t": t, "value": round(0.15 + 0.85 * wave * wave, 3),
                         "easing": "linear"})
        tracks.append({"id": "twinkle-%d" % index, "kind": "svg.opacity",
                       "src": "orbit.svg#twinkle-%d" % index, "keys": keys})

    svg_text = (
        '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" '
        'viewBox="0 0 480 270">'
        '<rect width="480" height="270" fill="#070a11"/>'
        + "".join(drawing) + "</svg>"
    )
    svg = WORK / "orbit.svg"
    svg.write_text(svg_text, encoding="ascii")

    scene = {
        "version": 1,
        "canvas": {"w": 480, "h": 270, "bg": "#070a11", "fps": fps},
        "timing": {"loop": "cycle", "rate": 1},
        "tracks": tracks,
    }
    scene_path = WORK / "orbit.json"
    scene_path.write_text(json.dumps(scene, indent=1) + "\n", encoding="ascii")
    run("tween", scene_path, "-o", out_dir / "demo-tween.gif")
    run("tween", scene_path, "-o", out_dir / "demo-tween.apng")
    run("tween", scene_path, "-o", out_dir / "demo-tween.svg")


def kinetic_render(name, drawing, scene):
    """Write one kinetic scene and render it to docs/demo-kinetic-<name>.gif."""
    folder = WORK / "kinetic"
    folder.mkdir(parents=True, exist_ok=True)
    (folder / (name + ".svg")).write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" '
        'viewBox="0 0 480 270"><rect width="480" height="270" fill="#000000"/>'
        + drawing + "</svg>", encoding="ascii")
    (folder / (name + ".json")).write_text(
        json.dumps(scene, indent=1) + "\n", encoding="ascii")
    run("tween", folder / (name + ".json"),
        "-o", DOCS / ("demo-kinetic-%s.gif" % name))


def kinetic_scene(duration, tracks):
    return {
        "version": 1,
        "canvas": {"w": 480, "h": 270, "bg": "#000000", "fps": 25},
        "timing": {"loop": "cycle", "rate": 1},
        "tracks": tracks,
    }


def kinetic_frames(duration):
    return list(range(0, duration, 40))


def smoothstep(u):
    u = max(0.0, min(1.0, u))
    return u * u * (3 - 2 * u)


def r2(value):
    return round(value, 2)


def r3(value):
    return round(value, 3)


def kinetic_drop():
    """One ball entering left, two decaying bounces, exit right."""
    duration = 1500
    floor = 232.0
    radius = 28.0
    start = (-40.0, 60.0)
    speed = 430.0
    gravity = 1400.0
    restitution = 0.55
    drop = floor - radius - start[1]
    t_fall = 1000.0 * math.sqrt(2 * drop / gravity)
    speed_down = gravity * t_fall / 1000.0
    bounce_one = speed_down * restitution
    flight_one = 1000.0 * 2 * bounce_one / gravity
    hold_one, hold_two = 80.0, 40.0
    bounce_two = bounce_one * restitution
    flight_two = 1000.0 * 2 * bounce_two / gravity
    t1 = t_fall
    t2 = t1 + hold_one + flight_one
    t3 = t2 + hold_two + flight_two

    def state(t):
        x = start[0] + speed * t / 1000.0
        if t < t1:
            u = t / 1000.0
            return x, start[1] + 0.5 * gravity * u * u, 0
        if t < t1 + hold_one:
            return x, floor - radius, 1
        if t < t2:
            u = (t - t1 - hold_one) / 1000.0
            return x, floor - radius - (bounce_one * u - 0.5 * gravity * u * u), 0
        if t < t2 + hold_two:
            return x, floor - radius, 2
        if t < t3:
            u = (t - t2 - hold_two) / 1000.0
            return x, floor - radius - (bounce_two * u - 0.5 * gravity * u * u), 0
        return x, floor - radius, 0

    x1 = start[0] + speed * t1 / 1000.0
    x2 = start[0] + speed * t2 / 1000.0
    drawing = (
        '<line x1="36" y1="%s" x2="444" y2="%s" stroke="#ffffff" stroke-width="2.5" '
        'stroke-linecap="round"/>'
        '<g transform="translate(%s,%s)"><circle id="ball" r="%s" fill="none" '
        'stroke="#ffffff" stroke-width="3.5"/></g>'
        '<g transform="translate(%s,%s)"><ellipse id="shock-one" rx="36" ry="9" '
        'fill="none" stroke="#ffffff" stroke-width="2" opacity="0"/></g>'
        '<g transform="translate(%s,%s)"><ellipse id="shock-two" rx="36" ry="9" '
        'fill="none" stroke="#ffffff" stroke-width="2" opacity="0"/></g>'
        % (floor, floor, start[0], start[1], radius, x1, floor - 2, x2, floor - 2)
    )
    times = kinetic_frames(duration)
    keys = []
    previous = 0
    for t in times:
        x, y, contact = state(t)
        if contact:
            sy = 0.78 if contact == 1 else 0.84
        elif previous:
            sy = 1.0 + (0.22 if previous == 1 else 0.16) * 0.6
        else:
            sy = 1.0
        if contact:
            y = floor - radius * sy
        sx = 1.0 / sy
        keys.append({"t": t, "translate": [r2(x - start[0]), r2(y - start[1])],
                     "scale": [round(sx, 4), round(sy, 4)], "easing": "linear"})
        previous = contact
    tracks = [{"id": "ball", "kind": "svg.transform", "src": "drop.svg#ball", "keys": keys}]
    for name, impact in (("shock-one", t1), ("shock-two", t2)):
        opacity = []
        scale = []
        for t in times:
            fade = 0.0
            grow = 0.4
            if impact <= t < impact + 260:
                fade = 0.9 * (1 - (t - impact) / 260.0)
                grow = 0.4 + 1.4 * (t - impact) / 260.0
            opacity.append({"t": t, "value": r3(fade)})
            scale.append({"t": t, "scale": [r3(grow), r3(grow)]})
        tracks.append({"id": name + "-opacity", "kind": "svg.opacity",
                       "src": "drop.svg#" + name, "keys": opacity})
        tracks.append({"id": name + "-scale", "kind": "svg.transform",
                       "src": "drop.svg#" + name, "keys": scale})
    kinetic_render("drop", drawing, kinetic_scene(duration, tracks))


def cradle_angle(t):
    # SVG rotation is clockwise on screen, so a positive angle moves a hanging
    # ball to the left. The lifted balls swing outward: left positive, right negative.
    left = 0.0
    if t < 400:
        left = 28.0 * math.cos(math.pi / 2 * t / 400)
    elif t < 1280:
        left = 0.0
    elif t < 1680:
        left = 28.0 * math.sin(math.pi / 2 * (t - 1280) / 400)
    else:
        left = 28.0
    right = 0.0
    if 440 <= t < 840:
        right = -28.0 * math.sin(math.pi / 2 * (t - 440) / 400)
    elif 840 <= t < 1240:
        right = -28.0 * math.cos(math.pi / 2 * (t - 840) / 400)
    return left, right


def kinetic_cradle():
    """Five balls exchanging an impulse through held contacts."""
    duration = 1720
    rail = 86.0
    length = 100.0
    radius = 18.0
    xs = [168.0, 204.0, 240.0, 276.0, 312.0]
    drawing = ('<line x1="118" y1="%s" x2="362" y2="%s" stroke="#7d838c" stroke-width="2.5" '
               'stroke-linecap="round"/>' % (rail, rail))
    for x in xs:
        drawing += '<circle cx="%s" cy="%s" r="3" fill="#7d838c"/>' % (x, rail)
    for i, x in enumerate(xs):
        ident = ' id="swing-%d"' % i if i in (0, 4) else ""
        drawing += ('<g transform="translate(%s,%s)"><g%s>'
                    '<line x1="0" y1="0" x2="0" y2="%s" stroke="#ffffff" stroke-width="2.5"/>'
                    '<circle cy="%s" r="%s" fill="none" stroke="#ffffff" stroke-width="3.5"/>'
                    "</g></g>" % (x, rail, ident, length, length, radius))
    tracks = []
    for index, side in ((0, 0), (4, 1)):
        keys = []
        for t in kinetic_frames(duration):
            left, right = cradle_angle(t)
            angle = left if side == 0 else right
            keys.append({"t": t, "rotate": r3(angle), "easing": "linear"})
        tracks.append({"id": "swing-%d-rotate" % index, "kind": "svg.transform",
                       "src": "cradle.svg#swing-%d" % index, "keys": keys})
    kinetic_render("cradle", drawing, kinetic_scene(duration, tracks))


LEAP_HALF = 1240
LEAP_REST_X, LEAP_LAND_X = 100.0, 340.0
LEAP_BODY_Y = 182.0
LEAP_HALF_BODY = 28.0
LEAP_PLATFORM = 210.0
LEAP_ANTENNA = LEAP_BODY_Y - LEAP_HALF_BODY


def leap_phase(t):
    half = 0 if t < LEAP_HALF else 1
    local = t % LEAP_HALF
    start_x, dir_x = (LEAP_REST_X, 1.0) if half == 0 else (LEAP_LAND_X, -1.0)
    end_x = LEAP_LAND_X if half == 0 else LEAP_REST_X
    if local < 80:
        return start_x, LEAP_BODY_Y, 1.0, 1.0
    if local < 280:
        u = smoothstep((local - 80) / 200.0)
        sy = 1.0 - 0.18 * u
        return start_x, LEAP_BODY_Y + 5 * u, 1.0 / sy, sy
    if local < 320:
        return start_x, LEAP_BODY_Y + 5, 1.0 / 0.82, 0.82
    if local < 360:
        u = (local - 320) / 40.0
        sy = 0.82 + 0.31 * u
        return start_x, LEAP_BODY_Y + 5 - 9 * u, 1.0 / sy, sy
    if local < 920:
        u = (local - 360) / 560.0
        x = start_x + dir_x * 240.0 * u
        y = LEAP_BODY_Y - 55.0 * 4 * u * (1 - u) - 4.0 * (1 - u)
        sy = 1.0 + 0.13 * max(0.0, 1.0 - u / 0.2)
        return x, y, 1.0 / sy, sy
    if local < 1040:
        return end_x, LEAP_PLATFORM - LEAP_HALF_BODY * 0.76, 1.0 / 0.76, 0.76
    if local < 1160:
        u = (local - 1040) / 120.0
        sy = 1.0 - 0.24 * math.exp(-4 * u) * math.cos(2 * math.pi * u)
        return end_x, LEAP_PLATFORM - LEAP_HALF_BODY * sy, 1.0 / sy, sy
    return end_x, LEAP_BODY_Y, 1.0, 1.0


def antenna_angle(t):
    half = 0 if t < LEAP_HALF else 1
    local = t % LEAP_HALF
    sign = 1.0 if half == 0 else -1.0
    if local < 80:
        angle = 0.0
    elif local < 280:
        angle = -20.0 * smoothstep((local - 80) / 200.0)
    elif local < 320:
        angle = -20.0
    elif local < 360:
        angle = -20.0 - 14.0 * (local - 320) / 40.0
    elif local < 920:
        u = (local - 360) / 560.0
        angle = -34.0 + 14.0 * min(1.0, u / 0.3)
    elif local < 1000:
        angle = 40.0
    elif local < 1160:
        u = (local - 1000) / 160.0
        angle = 40.0 * math.exp(-5 * u) * math.cos(2 * math.pi * u)
    else:
        angle = 0.0
    return sign * angle


def kinetic_leap():
    """Anticipation, a ballistic arc and a landing, with a lagging antenna."""
    duration = 2480
    drawing = (
        '<rect x="20" y="%s" width="160" height="14" fill="none" stroke="#ffffff" '
        'stroke-width="2.5"/>'
        '<rect x="300" y="%s" width="160" height="14" fill="none" stroke="#ffffff" '
        'stroke-width="2.5"/>'
        '<g transform="translate(%s,%s)"><g id="body">'
        '<rect x="-%s" y="-%s" width="%s" height="%s" rx="10" fill="none" '
        'stroke="#ffffff" stroke-width="4"/></g></g>'
        '<g transform="translate(%s,%s)"><g id="antenna">'
        '<line x1="0" y1="0" x2="0" y2="-36" stroke="#ffffff" stroke-width="3" '
        'stroke-linecap="round"/>'
        '<circle cy="-36" r="5" fill="#ffffff"/></g></g>'
        % (LEAP_PLATFORM, LEAP_PLATFORM,
           LEAP_REST_X, LEAP_BODY_Y, LEAP_HALF_BODY, LEAP_HALF_BODY,
           LEAP_HALF_BODY * 2, LEAP_HALF_BODY * 2,
           LEAP_REST_X, LEAP_ANTENNA)
    )
    body_keys = []
    antenna_keys = []
    for t in kinetic_frames(duration):
        x, y, sx, sy = leap_phase(t)
        body_keys.append({"t": t, "translate": [r2(x - LEAP_REST_X), r2(y - LEAP_BODY_Y)],
                          "scale": [round(sx, 4), round(sy, 4)], "easing": "linear"})
        pivot_y = y - LEAP_HALF_BODY * sy
        antenna_keys.append({"t": t,
                             "translate": [r2(x - LEAP_REST_X), r2(pivot_y - LEAP_ANTENNA)],
                             "rotate": r3(antenna_angle(t)), "easing": "linear"})
    tracks = [
        {"id": "body", "kind": "svg.transform", "src": "leap.svg#body", "keys": body_keys},
        {"id": "antenna", "kind": "svg.transform", "src": "leap.svg#antenna",
         "keys": antenna_keys},
    ]
    kinetic_render("leap", drawing, kinetic_scene(duration, tracks))


def kinetic_wave():
    """A damped pulse travelling through nine hanging rods."""
    duration = 1600
    rail = 88.0
    count = 9
    xs = [60.0 + i * 40.0 for i in range(count)]
    lengths = [70.0 + i * 10.0 for i in range(count)]
    drawing = ('<line x1="30" y1="%s" x2="430" y2="%s" stroke="#7d838c" stroke-width="2.5" '
               'stroke-linecap="round"/>' % (rail, rail))
    for i, (x, length) in enumerate(zip(xs, lengths)):
        drawing += ('<g transform="translate(%s,%s)"><g id="rod-%d">'
                    '<line x1="0" y1="0" x2="0" y2="%s" stroke="#ffffff" stroke-width="4" '
                    'stroke-linecap="round"/>'
                    '<circle cy="%s" r="7" fill="none" stroke="#ffffff" stroke-width="3"/>'
                    "</g></g>" % (x, rail, i, length, length))
    tracks = []
    for i in range(count):
        start = 80 + i * 40
        keys = []
        for t in kinetic_frames(duration):
            if t < start:
                angle = 0.0
            else:
                u = t - start
                angle = 22.0 * math.exp(-u / 250.0) * math.sin(2 * math.pi * u / 400.0)
            keys.append({"t": t, "rotate": r3(angle), "easing": "linear"})
        tracks.append({"id": "rod-%d-rotate" % i, "kind": "svg.transform",
                       "src": "wave.svg#rod-%d" % i, "keys": keys})
    kinetic_render("wave", drawing, kinetic_scene(duration, tracks))


def kinetic_samples():
    kinetic_drop()
    kinetic_cradle()
    kinetic_leap()
    kinetic_wave()


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
    tween_sample()
    kinetic_samples()
    for name in SAMPLES:
        path = DOCS / name
        print("%s  %.1f KB" % (name, path.stat().st_size / 1024.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
