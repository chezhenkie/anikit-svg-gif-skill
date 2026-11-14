#!/usr/bin/env python3
"""Render animated ASCII frames to a GIF.

Input is a text file with frames separated by a line containing only `---`.
A separator may carry a delay in ms: `--- 250`. ANSI SGR colors in the text
(30-37, 90-97, 38;5;n, 38;2;r;g;b) are drawn as colored glyphs. A directory
turns every .txt file into one frame, sorted by name.

Monospace font, fixed advance, full repaint per frame. Output is validated by
gifkit before the path is printed.
"""
import argparse
import os
import pathlib
import re
import sys

from PIL import Image, ImageDraw, ImageFont

import gifkit

SEP = re.compile(r"^---(?:\s+(\d+))?\s*$")
SGR = re.compile(r"\x1b\[([0-9;]*)m")
CSI = re.compile(r"\x1b\[[0-9;?]*[A-LN-Za-ln-z]")

BASE16 = [
    (0, 0, 0), (205, 49, 49), (13, 188, 121), (229, 229, 16),
    (36, 114, 200), (188, 63, 188), (17, 168, 205), (229, 229, 229),
    (102, 102, 102), (241, 76, 76), (35, 209, 139), (245, 245, 67),
    (59, 142, 234), (214, 112, 214), (41, 184, 219), (255, 255, 255),
]
CUBE = (0, 95, 135, 175, 215, 255)


def xterm_rgb(n):
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        return CUBE[n // 36], CUBE[(n // 6) % 6], CUBE[n % 6]
    v = 8 + 10 * (n - 232)
    return v, v, v


def parse_cells(line, default_fg):
    """One text line to (char, rgb) cells with ANSI SGR applied."""
    line = CSI.sub("", line.expandtabs(4))
    cells = []
    fg = default_fg
    pos = 0
    for m in SGR.finditer(line):
        for ch in line[pos:m.start()]:
            cells.append((ch, fg))
        params = [int(p) for p in m.group(1).split(";") if p != ""] or [0]
        i = 0
        while i < len(params):
            p = params[i]
            if p == 0:
                fg = default_fg
            elif 30 <= p <= 37:
                fg = BASE16[p - 30]
            elif 90 <= p <= 97:
                fg = BASE16[p - 90 + 8]
            elif p == 39:
                fg = default_fg
            elif p == 38 and i + 1 < len(params):
                mode = params[i + 1]
                if mode == 5 and i + 2 < len(params):
                    fg = xterm_rgb(max(0, min(255, params[i + 2])))
                    i += 2
                elif mode == 2 and i + 4 < len(params):
                    fg = tuple(max(0, min(255, v)) for v in params[i + 2:i + 5])
                    i += 4
                else:
                    i += 1
            i += 1
        pos = m.end()
    for ch in line[pos:]:
        cells.append((ch, fg))
    return cells


def load_frames(path, default_delay):
    """Text file or directory to [(lines, delay_ms)]."""
    path = pathlib.Path(path)
    if path.is_dir():
        files = sorted(p for p in path.iterdir() if p.suffix.lower() == ".txt")
        if not files:
            raise gifkit.GifError("no .txt frames in %s" % path)
        return [(p.read_text(encoding="utf-8").splitlines(), default_delay)
                for p in files]
    if not path.is_file():
        raise gifkit.GifError("input not found: %s" % path)

    frames = []
    lines = []
    delay = default_delay
    for raw in path.read_text(encoding="utf-8").splitlines():
        m = SEP.match(raw)
        if m:
            frames.append((lines, delay))
            lines = []
            delay = int(m.group(1)) if m.group(1) else default_delay
        else:
            lines.append(raw)
    if lines or not frames:
        frames.append((lines, delay))
    return frames


def find_font(explicit, size):
    """First existing font file from explicit path, env, then per-OS defaults."""
    candidates = []
    if explicit:
        candidates.append(explicit)
    if os.environ.get("ANIM_GIF_FONT"):
        candidates.append(os.environ["ANIM_GIF_FONT"])
    win = os.environ.get("WINDIR")
    if win:
        candidates += [
            os.path.join(win, "Fonts", "consola.ttf"),
            os.path.join(win, "Fonts", "CascadiaMono.ttf"),
        ]
    candidates += [
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansMono-Regular.ttf",
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.ttf",
    ]
    for candidate in candidates:
        if candidate and os.path.isfile(candidate):
            return ImageFont.truetype(candidate, size), candidate
    try:
        return ImageFont.load_default(size=size), "pillow default"
    except TypeError:
        return ImageFont.load_default(), "pillow default"


def render_frames(frames, font, line_h, advance, fg_default, bg, canvas=None):
    """Draw every frame at the global grid size, optionally centered on a fixed canvas."""
    grids = []
    cols = 0
    rows = 0
    for lines, _delay in frames:
        grid = [parse_cells(line, fg_default) for line in lines]
        grids.append(grid)
        cols = max(cols, max((len(line) for line in grid), default=0))
        rows = max(rows, len(grid))
    if cols == 0 or rows == 0:
        raise gifkit.GifError("frames are empty")

    w = max(1, int(round(cols * advance)))
    h = max(1, int(round(rows * line_h)))
    if canvas:
        cw, ch = canvas
        if cw < w or ch < h:
            raise gifkit.GifError(
                "canvas %dx%d is smaller than the %dx%d text grid" % (cw, ch, w, h))
        out_w, out_h = cw, ch
        ox, oy = (cw - w) // 2, (ch - h) // 2
    else:
        out_w, out_h = w, h
        ox, oy = 0, 0
    images = []
    for grid in grids:
        im = Image.new("RGB", (out_w, out_h), bg)
        d = ImageDraw.Draw(im)
        for r, line in enumerate(grid):
            y = oy + r * line_h
            for c, (ch, color) in enumerate(line):
                if ch == " ":
                    continue
                d.text((ox + c * advance, y), ch, font=font, fill=color)
        images.append(im)
    return images


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Render animated ASCII frames to a GIF.")
    p.add_argument("input", help="text file with --- separated frames, or a directory of .txt frames")
    p.add_argument("-o", "--out", default="ascii.gif", help="output GIF path")
    p.add_argument("--font", help="monospace TTF/TTC path")
    p.add_argument("--size", type=int, default=28, help="font size in px")
    p.add_argument("--width", type=int, help="fixed canvas width in px, centers the text grid")
    p.add_argument("--height", type=int, help="fixed canvas height in px, requires --width")
    p.add_argument("--fg", default="#e6edf6", help="default glyph color")
    p.add_argument("--bg", default="#0a0d12", help="canvas color")
    p.add_argument("--delay", type=int, default=100, help="default frame delay in ms")
    p.add_argument("--colors", type=int, default=255, help="palette size, max 255")
    p.add_argument("--dither", default="none", choices=sorted(gifkit.DITHERS))
    p.add_argument("--loop", type=int, default=0, help="0 loops forever")
    p.add_argument("--preview", help="optional PNG of the first frame")
    args = p.parse_args(argv)

    try:
        fg = gifkit.parse_color(args.fg)
        bg = gifkit.parse_color(args.bg)
        if (args.width is None) != (args.height is None):
            raise gifkit.GifError("--width and --height must be given together")
        canvas = None
        if args.width is not None:
            if args.width < 8 or args.height < 8:
                raise gifkit.GifError("canvas width and height must be at least 8")
            canvas = (args.width, args.height)
        frames = load_frames(args.input, max(gifkit.MIN_DELAY_MS, args.delay))
        font, font_name = find_font(args.font, args.size)
        if "default" in font_name:
            print("WARN using Pillow default font, pass --font for a stable grid",
                  file=sys.stderr)
        ascent, descent = font.getmetrics()
        line_h = ascent + descent
        advance = font.getlength("M")
        if abs(font.getlength("i") - advance) > 0.5:
            print("WARN font is not monospace, columns may not line up",
                  file=sys.stderr)
        images = render_frames(frames, font, line_h, advance, fg, bg, canvas)
        delays = [delay for _lines, delay in frames]
        result = gifkit.write_gif(images, args.out, delays, loop=args.loop,
                                  colors=args.colors, dither=args.dither)
        if args.preview:
            pathlib.Path(args.preview).parent.mkdir(parents=True, exist_ok=True)
            images[0].save(args.preview)
            print("%s  still frame" % args.preview)
        print("%s  %d frames, %.2f s, %d x %d"
              % (result.path.name, result.frames, result.total_ms / 1000.0,
                 result.size[0], result.size[1]))
    except (gifkit.GifError, OSError, ValueError) as exc:
        print("FAIL %s" % exc, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
