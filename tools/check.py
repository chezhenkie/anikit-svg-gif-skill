"""Route and hygiene checks for the animated-gif skill.

Runs each tool on generated fixtures, verifies the written GIF (frame count,
duration, size, motion), then scans the repository text for secrets, absolute
paths, non-ASCII characters and CRLF endings. Prints one PASS, SKIP or FAIL
line per check. Optional dependencies skip instead of failing unless --strict
is passed.
"""
import hashlib
import pathlib
import re
import shutil
import subprocess
import sys

from PIL import Image, ImageChops, ImageDraw

ROOT = pathlib.Path(__file__).resolve().parent.parent
BIN = ROOT / "agent-skill" / "bin"
WORK = ROOT / "scratch" / "checks"
PY = sys.executable
ESC = chr(27)


class Fail(Exception):
    pass


class Skip(Exception):
    pass


def run(script, *args, expect=0):
    cmd = [PY, str(BIN / script)] + [str(a) for a in args]
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    if proc.returncode != expect:
        raise Fail("%s exit %d: %s" % (script, proc.returncode,
                                       (proc.stderr or proc.stdout).strip()[-300:]))
    return proc


def gif_info(path):
    im = Image.open(path)
    count = 0
    total = 0
    frames = []
    try:
        while True:
            count += 1
            total += int(im.info.get("duration", 0) or 0)
            frames.append(im.convert("RGB").copy())
            try:
                im.seek(im.tell() + 1)
            except EOFError:
                break
        size = im.size
    finally:
        im.close()
    return count, total, size, frames


def require(cond, message):
    if not cond:
        raise Fail(message)


def test_ascii():
    src = WORK / "frames.txt"
    src.write_text(
        "  ( )   ( )\n (   ) (   )\n  ( )   ( )\n---\n"
        "   \\  |  /\n    \\ | /\n  ---   ---\n    / | \\\n   /  |  \\\n---\n"
        + ESC + "[38;5;46m ##### " + ESC + "[0m\n"
        + ESC + "[38;5;46m##   ##" + ESC + "[0m\n"
        + ESC + "[38;5;46m ##### " + ESC + "[0m\n",
        encoding="utf-8")
    out = WORK / "ascii.gif"
    run("ascii_gif.py", src, "-o", out, "--size", 24)
    count, total, size, frames = gif_info(out)
    require(count == 3, "ascii frames %d" % count)
    require(total == 300, "ascii duration %d" % total)
    require(size[0] > 100 and size[1] > 100, "ascii size %s" % (size,))
    green = frames[2].getextrema()
    require(green[1][1] > 200 and green[0][1] < 120, "ascii ansi color not applied")

    again = WORK / "ascii-again.gif"
    run("ascii_gif.py", src, "-o", again, "--size", 24)
    require(hashlib.sha256(out.read_bytes()).digest()
            == hashlib.sha256(again.read_bytes()).digest(), "ascii not deterministic")

    fixed = WORK / "ascii-fixed.gif"
    run("ascii_gif.py", src, "-o", fixed, "--size", 24,
        "--width", 320, "--height", 160)
    _fc, _ft, fsize, _ff = gif_info(fixed)
    require(fsize == (320, 160), "ascii fixed canvas %s" % (fsize,))


def make_pics(count=3):
    folder = WORK / "pics"
    folder.mkdir(parents=True, exist_ok=True)
    colors = [(220, 60, 60), (60, 180, 90), (70, 110, 220)]
    for i in range(count):
        im = Image.new("RGB", (640, 480), colors[i])
        d = ImageDraw.Draw(im)
        d.rectangle((40, 40, 600, 440), outline=(255, 255, 255), width=6)
        im.save(folder / ("pic-%02d.png" % (i + 1)))
    return folder, colors


def test_pics():
    folder, colors = make_pics()
    out = WORK / "pics-seq.gif"
    run("pics_gif.py", folder, "-o", out, "--mode", "sequence",
        "--fps", 10, "--hold", 0.5, "--width", 320)
    count, total, size, frames = gif_info(out)
    require(count == 3, "pics sequence frames %d" % count)
    require(total == 1500, "pics sequence duration %d" % total)
    require(size == (320, 240), "pics size %s" % (size,))
    for i, want in enumerate(colors):
        px = frames[i].getpixel((160, 120))
        require(abs(px[0] - want[0]) < 20 and abs(px[1] - want[1]) < 20
                and abs(px[2] - want[2]) < 20,
                "pics frame %d color %s" % (i + 1, px))

    fade = WORK / "pics-fade.gif"
    run("pics_gif.py", folder, "-o", fade, "--mode", "crossfade",
        "--fps", 10, "--hold", 0.5, "--transition", 0.3, "--width", 320)
    fcount, ftotal, _fsize, _fframes = gif_info(fade)
    require(fcount == 12, "pics crossfade frames %d" % fcount)
    require(ftotal == 2400, "pics crossfade duration %d" % ftotal)


def svg_fixture(path):
    path.write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" width="320" height="240" '
        'viewBox="0 0 320 240">'
        '<rect width="320" height="240" fill="#101820"/>'
        '<circle cx="60" cy="120" r="28" fill="#ff6644">'
        '<animate attributeName="cx" values="60;260;60" dur="2s" '
        'repeatCount="indefinite"/></circle>'
        '<rect id="box" x="135" y="95" width="50" height="50" fill="#44aaff"/>'
        '</svg>', encoding="utf-8")


def differing(a, b):
    return ImageChops.difference(a, b).getbbox() is not None


def node_env():
    import os
    env = os.environ.copy()
    npm = shutil.which("npm")
    if npm:
        proc = subprocess.run([npm, "root", "-g"], capture_output=True,
                              text=True, timeout=30)
        root = proc.stdout.strip()
        if proc.returncode == 0 and root:
            env["NODE_PATH"] = os.pathsep.join(
                p for p in (root, env.get("NODE_PATH", "")) if p)
    return env


def test_svg():
    node = shutil.which("node")
    if not node:
        raise Skip("node not installed")
    probe = subprocess.run([node, "-e", "require('playwright')"],
                           capture_output=True, text=True,
                           env=node_env(), timeout=60)
    if probe.returncode != 0:
        raise Skip("playwright not installed for node")
    svg = WORK / "anim.svg"
    svg_fixture(svg)
    out = WORK / "svg-smil.gif"
    try:
        run("svg_gif.py", svg, "-o", out, "--duration", 1, "--fps", 8)
    except Fail as exc:
        if "Executable doesn't exist" in str(exc) or "chromium" in str(exc).lower():
            raise Skip("playwright browser not installed")
        raise
    count, total, size, frames = gif_info(out)
    require(count == 8, "svg frames %d" % count)
    require(total == 960, "svg duration %d" % total)
    require(size == (320, 240), "svg size %s" % (size,))
    require(differing(frames[0], frames[-1]), "svg smil did not move")

    preset = WORK / "svg-preset.gif"
    run("svg_gif.py", svg, "-o", preset, "--duration", 1, "--fps", 8,
        "--animate", "rotate", "--selector", "#box")
    pcount, _ptotal, _psize, pframes = gif_info(preset)
    require(pcount == 8, "svg preset frames %d" % pcount)
    require(differing(pframes[0], pframes[4]), "svg preset did not move")


def test_errors():
    bad = WORK / "missing.txt"
    proc = run("ascii_gif.py", bad, "-o", WORK / "nope.gif", expect=1)
    require("FAIL" in proc.stderr, "ascii error did not print FAIL")
    require(not (WORK / "nope.gif").exists(), "failed run left an output file")
    left = list(WORK.glob("*.tmp.gif"))
    require(not left, "temp files left: %s" % left)


SECRETS = re.compile(
    r"(ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9]{20,}"
    r"|AKIA[0-9A-Z]{16}|BEGIN [A-Z ]*PRIVATE KEY|password\s*[:=]\s*\S+)",
    re.IGNORECASE)
MACHINE_PATHS = re.compile(
    r"([A-Za-z]:\\|(?<![A-Za-z])[A-Za-z]:/|/home/|/Users/|ocdev|lap1user|westercoder@)",
    re.IGNORECASE)


def repo_text_files():
    files = [ROOT / ".gitattributes", ROOT / ".gitignore", ROOT / "README.md",
             ROOT / "CHANGELOG.md", ROOT / "LICENSE"]
    for folder in ("agent-skill", "tools", "docs"):
        files += sorted((ROOT / folder).rglob("*")) if (ROOT / folder).is_dir() else []
    text = []
    for p in files:
        if p.is_file() and p.suffix.lower() in (
                ".md", ".py", ".cjs", ".js", ".json", ".txt", ".html",
                ".css", "", ".gitignore", ".gitattributes"):
            if "__pycache__" not in p.parts:
                text.append(p)
    return text


def test_hygiene():
    skill = ROOT / "agent-skill" / "SKILL.md"
    require(skill.is_file(), "agent-skill/SKILL.md missing")
    front = skill.read_text(encoding="utf-8")
    match = re.search(r"^name:\s*(\S+)", front, re.MULTILINE)
    require(match and match.group(1) == "animated-gif",
            "SKILL.md name is not animated-gif")
    readme = ROOT / "README.md"
    require(readme.is_file(), "README.md missing")
    require("`animated-gif`" in readme.read_text(encoding="utf-8"),
            "README does not name the skill folder")

    for p in repo_text_files():
        raw = p.read_bytes()
        require(b"\r\n" not in raw, "%s uses CRLF" % p.relative_to(ROOT))
        try:
            text = raw.decode("ascii")
        except UnicodeDecodeError as exc:
            raise Fail("%s is not ASCII: %s" % (p.relative_to(ROOT), exc))
        for pattern, label in ((SECRETS, "secret"), (MACHINE_PATHS, "absolute path")):
            if label == "absolute path" and p.resolve() == pathlib.Path(__file__).resolve():
                continue
            found = pattern.search(text)
            require(not found, "%s in %s" % (label, p.relative_to(ROOT)))

    for target in re.findall(r"\]\((docs/[^)]+)\)", readme.read_text(encoding="utf-8")):
        require((ROOT / target).is_file(), "README link missing: %s" % target)


TESTS = [
    ("ascii route", test_ascii),
    ("pics route", test_pics),
    ("svg route", test_svg),
    ("error contract", test_errors),
    ("repository hygiene", test_hygiene),
]


def main():
    strict = "--strict" in sys.argv[1:]
    if WORK.exists():
        shutil.rmtree(WORK)
    WORK.mkdir(parents=True)
    failed = 0
    skipped = 0
    for name, fn in TESTS:
        try:
            fn()
            print("PASS %s" % name)
        except Skip as exc:
            skipped += 1
            print("SKIP %s: %s" % (name, exc))
        except Exception as exc:
            failed += 1
            print("FAIL %s: %s" % (name, exc))
    if failed or (strict and skipped):
        print("%d failed, %d skipped" % (failed, skipped))
        return 1
    print("passed %d, skipped %d" % (len(TESTS) - skipped, skipped))
    return 0


if __name__ == "__main__":
    sys.exit(main())
