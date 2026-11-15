#!/usr/bin/env python3
"""Animate an SVG or HTML page into a GIF.

Frame capture runs through Playwright with an installed Chromium based browser
(bin/render_frames.cjs); a browser downloaded by Playwright is the fallback. CSS
animations and SMIL are paused and seeked to explicit times, so frames do not
depend on wall-clock timing. Presets rotate, pulse and fade can be applied to
the whole drawing or to one CSS selector.

For .svg input the file is wrapped in an HTML page at the requested viewport;
for .html input the page is opened as is, and its own CSS or SMIL animations
are sampled. JS-driven canvas pages need their own frame hook and are not
driven by this tool.
"""
import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

from PIL import Image

import gifkit

MAX_FPS = 50
PRESETS = ("none", "rotate", "pulse", "fade")
WINDOWS_BROWSERS = (
    r"%LOCALAPPDATA%\Chromium\Application\chrome.exe",
    r"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe",
    r"%PROGRAMFILES%\Google\Chrome\Application\chrome.exe",
    r"%PROGRAMFILES(X86)%\Google\Chrome\Application\chrome.exe",
    r"%PROGRAMFILES(X86)%\Microsoft\Edge\Application\msedge.exe",
)
MAC_BROWSERS = (
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
)
LINUX_BROWSERS = ("ungoogled-chromium", "chromium", "chromium-browser",
                  "google-chrome", "google-chrome-stable")


def svg_size(text):
    """Read width, height and viewBox from the root svg element."""
    match = re.search(r"<svg\b[^>]*>", text, re.IGNORECASE | re.DOTALL)
    if not match:
        raise gifkit.GifError("no <svg> element found")
    attrs = {}
    for name, value in re.findall(r'([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*"([^"]*)"',
                                  match.group(0)):
        attrs[name.lower()] = value
    for name, value in re.findall(r"([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*'([^']*)'",
                                  match.group(0)):
        attrs.setdefault(name.lower(), value)

    def number(value):
        m = re.match(r"\s*([0-9]+(?:\.[0-9]+)?)", value or "")
        return float(m.group(1)) if m else None

    w = number(attrs.get("width"))
    h = number(attrs.get("height"))
    vw = vh = None
    box = attrs.get("viewbox")
    if box:
        parts = [p for p in re.split(r"[,\s]+", box.strip()) if p]
        if len(parts) == 4:
            try:
                vw, vh = float(parts[2]), float(parts[3])
            except ValueError:
                vw = vh = None
    return w, h, vw, vh


def resolve_size(input_path, width, height):
    if input_path.suffix.lower() != ".svg":
        return width or 800, height or 600
    w, h, vw, vh = svg_size(input_path.read_text(encoding="utf-8"))
    base_w = w or vw
    base_h = h or vh
    if width and height:
        return width, height
    if width:
        if base_w and base_h:
            return width, max(8, round(width * base_h / base_w))
        return width, 600
    if height:
        if base_w and base_h:
            return max(8, round(height * base_w / base_h)), height
        return 800, height
    if base_w and base_h:
        return max(8, round(base_w)), max(8, round(base_h))
    return 800, 600


def find_browser():
    """Installed Chromium based browser, or None.

    The Playwright download is only needed when no system browser is found.
    GIFKIT_BROWSER overrides the search and must point at an existing file.
    """
    custom = os.environ.get("GIFKIT_BROWSER")
    if custom:
        path = pathlib.Path(os.path.expandvars(custom)).expanduser()
        if path.is_file():
            return path
        raise gifkit.GifError("GIFKIT_BROWSER does not exist: %s" % path)
    if sys.platform == "win32":
        candidates = WINDOWS_BROWSERS
    elif sys.platform == "darwin":
        candidates = MAC_BROWSERS
    else:
        candidates = ()
    for candidate in candidates:
        path = pathlib.Path(os.path.expandvars(candidate)).expanduser()
        if path.is_file():
            return path
    for name in LINUX_BROWSERS:
        found = shutil.which(name)
        if found:
            return pathlib.Path(found)
    return None


def node_env():
    """Environment with the global npm module root on NODE_PATH."""
    env = os.environ.copy()
    npm = shutil.which("npm")
    if npm:
        for cmd in ([npm, "root", "-g"], "npm root -g"):
            try:
                proc = subprocess.run(cmd, capture_output=True, text=True,
                                      timeout=30, shell=isinstance(cmd, str))
            except (OSError, subprocess.TimeoutExpired):
                continue
            root = proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else ""
            if proc.returncode == 0 and root:
                env["NODE_PATH"] = os.pathsep.join(
                    p for p in (root, env.get("NODE_PATH", "")) if p)
                break
    return env


def check_playwright(node, env):
    try:
        probe = subprocess.run([node, "-e", "require('playwright')"],
                               capture_output=True, text=True, env=env, timeout=60)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise gifkit.GifError("playwright probe failed: %s" % exc)
    if probe.returncode != 0:
        raise gifkit.GifError(
            "playwright is not reachable from node; run: npm install -g playwright")


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Animate an SVG or HTML page into a GIF.")
    p.add_argument("input", help=".svg or .html file")
    p.add_argument("-o", "--out", default="svg.gif", help="output GIF path")
    p.add_argument("--width", type=int, help="viewport width, default from the drawing")
    p.add_argument("--height", type=int, help="viewport height, default from the drawing")
    p.add_argument("--duration", type=float, default=2.0,
                   help="seconds per animation cycle")
    p.add_argument("--fps", type=int, default=15, help="frames per second, max 50")
    p.add_argument("--animate", default="none", choices=PRESETS,
                   help="preset motion; none uses the animation already in the file")
    p.add_argument("--selector", help="CSS selector for the preset target, default the svg root")
    p.add_argument("--background", default="#ffffff", help="page color behind the drawing")
    p.add_argument("--dpr", type=int, default=1, help="device pixel ratio, 1 or 2")
    p.add_argument("--colors", type=int, default=255, help="palette size, max 255")
    p.add_argument("--dither", default="none", choices=sorted(gifkit.DITHERS))
    p.add_argument("--loop", type=int, default=0, help="0 loops forever")
    p.add_argument("--preview", help="optional PNG of the first frame")
    p.add_argument("--browser",
                   help="path to a Chromium based browser; default is an installed one")
    args = p.parse_args(argv)

    try:
        input_path = pathlib.Path(args.input)
        if not input_path.is_file():
            raise gifkit.GifError("input not found: %s" % input_path)
        if input_path.suffix.lower() not in (".svg", ".html", ".htm"):
            raise gifkit.GifError("input must be .svg, .html or .htm")
        if args.duration <= 0:
            raise gifkit.GifError("duration must be greater than 0")
        if not 1 <= args.fps <= MAX_FPS:
            raise gifkit.GifError("fps must be between 1 and %d" % MAX_FPS)
        if args.dpr not in (1, 2):
            raise gifkit.GifError("dpr must be 1 or 2")
        bg = gifkit.parse_color(args.background, (255, 255, 255))

        width, height = resolve_size(input_path, args.width, args.height)
        if width < 8 or height < 8:
            raise gifkit.GifError("width and height must be at least 8")

        node = shutil.which("node")
        if not node:
            raise gifkit.GifError("node not found; Playwright needs Node.js")
        env = node_env()
        check_playwright(node, env)
        if args.browser:
            browser = pathlib.Path(args.browser).expanduser()
            if not browser.is_file():
                raise gifkit.GifError("browser not found: %s" % browser)
        else:
            browser = find_browser()

        count = max(2, round(args.duration * args.fps))
        out = pathlib.Path(args.out)
        out.parent.mkdir(parents=True, exist_ok=True)
        work = pathlib.Path(tempfile.mkdtemp(prefix=".gifframes-", dir=out.parent))
        try:
            frames = []
            for i in range(count):
                frames.append({
                    "t": i / float(args.fps),
                    "path": str(work / ("frame-%04d.png" % i)),
                })
            plan = {
                "input": str(input_path.resolve()),
                "isSvg": input_path.suffix.lower() == ".svg",
                "baseUrl": input_path.resolve().parent.as_uri() + "/",
                "width": width,
                "height": height,
                "dpr": args.dpr,
                "background": "#%02x%02x%02x" % bg,
                "selector": args.selector,
                "animate": args.animate,
                "duration": args.duration,
                "executablePath": str(browser) if browser else None,
                "frames": frames,
            }
            plan_path = work / "plan.json"
            plan_path.write_text(json.dumps(plan), encoding="utf-8")
            helper = pathlib.Path(__file__).with_name("render_frames.cjs")
            try:
                proc = subprocess.run([node, str(helper), str(plan_path)],
                                      capture_output=True, text=True, env=env,
                                      timeout=600)
            except subprocess.TimeoutExpired:
                raise gifkit.GifError("frame capture timed out")
            except OSError as exc:
                raise gifkit.GifError("frame capture failed: %s" % exc)
            if proc.returncode != 0:
                detail = (proc.stderr or proc.stdout or "").strip()
                lines = [ln.strip() for ln in detail.splitlines() if ln.strip()]
                message = next((ln for ln in lines if ln.startswith("FAIL")),
                               lines[-1] if lines else "node error")
                raise gifkit.GifError("frame capture failed: %s" % message)
            pngs = sorted(work.glob("frame-*.png"))
            if len(pngs) != count:
                raise gifkit.GifError("captured %d of %d frames" % (len(pngs), count))
            images = []
            for png in pngs:
                with Image.open(png) as im:
                    images.append(gifkit.flatten(im.convert("RGBA"), bg))
            delays = gifkit.uniform_durations(count, args.fps)
            result = gifkit.write_gif(images, out, delays, loop=args.loop,
                                      colors=args.colors, dither=args.dither)
            if args.preview:
                pathlib.Path(args.preview).parent.mkdir(parents=True, exist_ok=True)
                images[0].save(args.preview)
                print("%s  still frame" % args.preview)
        finally:
            shutil.rmtree(work, ignore_errors=True)

        print("%s  %d frames, %.2f s, %d x %d"
              % (result.path.name, result.frames, result.total_ms / 1000.0,
                 result.size[0], result.size[1]))
    except (gifkit.GifError, OSError, ValueError) as exc:
        print("FAIL %s" % exc, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
