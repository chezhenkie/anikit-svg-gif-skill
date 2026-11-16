"""Generate the sample animations used by the README.

Creates fixtures under scratch/docs and writes the four demo animations into
docs/, plus the keyframe sample as APNG and as an animated SVG, and four white
line kinetic studies on black.
Run from the repository root, after building the binary:

    python tools/make_docs.py

The binary is taken from ANIKIT, then agent-skill/bin, then a local cargo build.
Only the standard library is used, so no Python packages are needed.
"""
import json
import math
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


def tween_sample():
    svg = WORK / "dots.svg"
    svg.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="270" '
        'viewBox="0 0 480 270">'
        '<rect width="480" height="270" fill="#0b0d12"/>'
        '<circle id="dot-a" cx="150" cy="160" r="24" fill="#ff7a66"/>'
        '<circle id="dot-b" cx="240" cy="160" r="24" fill="#7aa2ff"/>'
        '<circle id="dot-c" cx="330" cy="160" r="24" fill="#45c2b0"/>'
        '</svg>', encoding="ascii")
    spring = {"stiffness": 400, "damping": 32, "mass": 1, "settle": 560}
    tracks = []
    for name, start in zip("abc", (0, 100, 200)):
        tracks.append({
            "id": "dot-%s" % name,
            "kind": "svg.transform",
            "src": "dots.svg#dot-%s" % name,
            "keys": [
                {"t": start, "translate": [0, 0], "easing": {"spring": spring}},
                {"t": start + 600, "translate": [0, -56], "easing": "hold"},
                {"t": start + 900, "translate": [0, -56], "easing": {"spring": spring}},
                {"t": start + 1500, "translate": [0, 0]},
            ],
        })
    scene = {
        "version": 1,
        "canvas": {"w": 480, "h": 270, "bg": "#0b0d12", "fps": 25},
        "timing": {"loop": "cycle", "rate": 1},
        "tracks": tracks,
    }
    scene_path = WORK / "dots.json"
    scene_path.write_text(json.dumps(scene, indent=2) + "\n", encoding="ascii")
    run("tween", scene_path, "-o", DOCS / "demo-tween.gif")
    run("tween", scene_path, "-o", DOCS / "demo-tween.apng")
    run("tween", scene_path, "-o", DOCS / "demo-tween.svg")


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


def drop_events(release, restitution, hold_first, hold_later, height, gravity=1400.0):
    """Flights and contacts under gravity, times in milliseconds."""
    events = [("hold", 0, release, height)]
    t = release
    t_fall = 1000.0 * math.sqrt(2 * height / gravity)
    events.append(("fall", t, t + t_fall, height))
    t += t_fall
    events.append(("hold", t, t + hold_first, 0.0))
    t += hold_first
    apex = height * restitution ** 2
    while apex > 1.0:
        flight = 1000.0 * 2 * math.sqrt(2 * apex / gravity)
        events.append(("bounce", t, t + flight, apex))
        t += flight
        events.append(("hold", t, t + hold_later, 0.0))
        t += hold_later
        apex *= restitution ** 2
    events.append(("hold", t, 10 ** 9, 0.0))
    return events


def drop_height(events, t):
    for kind, start, end, apex in events:
        if start <= t < end:
            if kind == "hold":
                return apex
            u = (t - start) / (end - start)
            if kind == "fall":
                return apex * (1 - u * u)
            return apex * 4 * u * (1 - u)
    return 0.0


def contact_at(events, t):
    return any(kind == "hold" and apex == 0 and start <= t < end
               for kind, start, end, apex in events)


def contacts(events):
    return [start for kind, start, _, apex in events if kind == "hold" and apex == 0]


def contact_start(events, t):
    for kind, start, end, apex in events:
        if kind == "hold" and apex == 0 and start <= t < end:
            return start
    return None


def drop_track(name, radius, floor, top, events, squash, times):
    keys = []
    for index, t in enumerate(times):
        start = contact_start(events, t)
        after = index > 0 and contact_at(events, times[index - 1]) and start is None
        if start is not None and t < start + 2 * 40:
            sy = squash
        elif start is not None:
            sy = 1.0
        elif after:
            sy = 1.0 + (1.0 - squash) * 0.6
        else:
            sy = 1.0
        sx = 1.0 / sy
        height = drop_height(events, t)
        if start is not None:
            centre = floor - radius * sy
        else:
            centre = floor - radius - height
        keys.append({"t": t, "translate": [0, r2(centre - top)],
                     "scale": [round(sx, 4), round(sy, 4)], "easing": "linear"})
    return {"id": name, "kind": "svg.transform", "src": "drop.svg#%s" % name, "keys": keys}


def shock_tracks(name, impact_times, times):
    base = {"src": "drop.svg#" + name}
    opacity = []
    scale = []
    for t in times:
        fade = 0.0
        grow = 0.4
        for impact in impact_times:
            if impact <= t < impact + 260:
                fade = max(fade, 0.9 * (1 - (t - impact) / 260.0))
                grow = max(grow, 0.4 + 1.4 * (t - impact) / 260.0)
        opacity.append({"t": t, "value": r3(fade)})
        scale.append({"t": t, "scale": [r3(grow), r3(grow)]})
    return [
        dict(base, id=name + "-opacity", kind="svg.opacity", keys=opacity),
        dict(base, id=name + "-scale", kind="svg.transform", keys=scale),
    ]


def kinetic_drop():
    """Gravity, contact squash and bounce decay at two restitutions."""
    duration = 2800
    release = 200
    floor = 232.0
    radius = 32.0
    height = 150.0
    specs = [
        ("ball-a", 150.0, 0.55, 80, 40, 0.78),
        ("ball-b", 330.0, 0.35, 120, 40, 0.84),
    ]
    events = {n: drop_events(release, e, hf, hl, height) for n, _, e, hf, hl, _ in specs}
    drawing = (
        '<line x1="36" y1="%s" x2="444" y2="%s" stroke="#ffffff" stroke-width="2.5" '
        'stroke-linecap="round"/>'
        '<g id="stage">' % (floor, floor)
    )
    for name, x, _, _, _, _ in specs:
        drawing += ('<g transform="translate(%s,50)"><circle id="%s" r="%s" fill="none" '
                    'stroke="#ffffff" stroke-width="3.5"/></g>'
                    % (x, name, radius))
        drawing += ('<g transform="translate(%s,%s)"><ellipse id="%s-ring" rx="40" ry="9" '
                    'fill="none" stroke="#ffffff" stroke-width="2" opacity="0"/></g>'
                    % (x, floor - 2, name))
    drawing += "</g>"
    times = kinetic_frames(duration)
    tracks = []
    for name, _, _, _, _, squash in specs:
        tracks.append(drop_track(name, radius, floor, 50.0, events[name], squash, times))
        tracks.extend(shock_tracks(name + "-ring", contacts(events[name]), times))
    tracks.append({"id": "stage-fade", "kind": "svg.opacity", "src": "drop.svg#stage",
                   "keys": [{"t": 0, "value": 0}, {"t": 200, "value": 1},
                            {"t": 2560, "value": 1}, {"t": 2800, "value": 0}]})
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
