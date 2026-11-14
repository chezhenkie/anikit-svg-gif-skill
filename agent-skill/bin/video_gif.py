#!/usr/bin/env python3
"""Convert a video clip to an animated GIF with ffmpeg frames and gifkit.

ffmpeg extracts PNG frames at the requested rate and width; gifkit builds one
shared palette, writes the GIF and verifies frame count, duration and size.
No temporary files are left behind.
"""
import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

from PIL import Image

import gifkit

MAX_FPS = 50


def find_ffmpeg():
    exe = shutil.which("ffmpeg")
    if not exe:
        raise gifkit.GifError("ffmpeg not found on PATH")
    return exe


def extract_frames(ffmpeg, input_path, work, width, fps, start, duration):
    cmd = [ffmpeg, "-hide_banner", "-loglevel", "error", "-y"]
    if start:
        cmd += ["-ss", str(start)]
    cmd += ["-i", str(input_path)]
    if duration:
        cmd += ["-t", str(duration)]
    cmd += ["-vf", "fps=%s,scale=%d:-2:flags=lanczos" % (fps, width),
            "-pix_fmt", "rgb24", str(work / "frame-%05d.png")]
    try:
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        raise gifkit.GifError("ffmpeg timed out")
    except OSError as exc:
        raise gifkit.GifError("ffmpeg failed to start: %s" % exc)
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout or "").strip()
        raise gifkit.GifError("ffmpeg failed: %s"
                              % (detail.splitlines()[-1] if detail else "exit %d" % proc.returncode))


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Convert a video clip to an animated GIF.")
    p.add_argument("input", help="video file readable by ffmpeg")
    p.add_argument("-o", "--out", default="clip.gif", help="output GIF path")
    p.add_argument("--fps", type=int, default=12, help="frames per second, max 50")
    p.add_argument("--width", type=int, default=800, help="output width in px")
    p.add_argument("--start", type=float, default=0.0, help="start time in seconds")
    p.add_argument("--duration", type=float, help="clip length in seconds, default to the end")
    p.add_argument("--colors", type=int, default=255, help="palette size, max 255")
    p.add_argument("--dither", default="none", choices=sorted(gifkit.DITHERS))
    p.add_argument("--loop", type=int, default=0, help="0 loops forever")
    p.add_argument("--preview", help="optional PNG of the first frame")
    args = p.parse_args(argv)

    try:
        input_path = pathlib.Path(args.input)
        if not input_path.is_file():
            raise gifkit.GifError("input not found: %s" % input_path)
        if not 1 <= args.fps <= MAX_FPS:
            raise gifkit.GifError("fps must be between 1 and %d" % MAX_FPS)
        if args.width < 8:
            raise gifkit.GifError("width must be at least 8")
        if args.start < 0:
            raise gifkit.GifError("start must be 0 or more")
        if args.duration is not None and args.duration <= 0:
            raise gifkit.GifError("duration must be greater than 0")
        ffmpeg = find_ffmpeg()

        out = pathlib.Path(args.out)
        out.parent.mkdir(parents=True, exist_ok=True)
        work = pathlib.Path(tempfile.mkdtemp(prefix=".gifframes-", dir=out.parent))
        try:
            extract_frames(ffmpeg, input_path, work, args.width, args.fps,
                           args.start, args.duration)
            pngs = sorted(work.glob("frame-*.png"))
            if not pngs:
                raise gifkit.GifError("ffmpeg produced no frames")
            images = []
            for png in pngs:
                with Image.open(png) as im:
                    images.append(im.convert("RGB"))
            delays = gifkit.uniform_durations(len(images), args.fps)
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
