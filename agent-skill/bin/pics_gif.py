#!/usr/bin/env python3
"""Turn a folder of images into an animated GIF.

Modes:
  sequence  - each image holds for --hold seconds
  crossfade - hold, then blend into the next image
  kenburns  - slow zoom on each image, optional crossfade

Images are EXIF-rotated, fitted to one canvas, quantized onto a single shared
palette and written through gifkit, which verifies the result before printing
the path.
"""
import argparse
import pathlib
import re
import sys

from PIL import Image, ImageOps

import gifkit

IMAGE_EXTS = {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".tif", ".tiff", ".gif"}
MAX_FPS = 50


def natural_key(path):
    parts = re.split(r"(\d+)", path.name.lower())
    return [int(p) if p.isdigit() else p for p in parts]


def load_images(folder, skip):
    files = [p for p in folder.iterdir()
             if p.is_file() and p.suffix.lower() in IMAGE_EXTS
             and not p.name.startswith(".") and p.resolve() != skip]
    files.sort(key=natural_key)
    if not files:
        raise gifkit.GifError("no images in %s" % folder)
    images = []
    for p in files:
        with Image.open(p) as im:
            im = ImageOps.exif_transpose(im)
            images.append(im.convert("RGB"))
    return images


def target_height(width, first, explicit):
    if explicit:
        return explicit
    return max(2, round(width / (first.width / float(first.height))))


def cover_base(im, size):
    """2x cover fit, so Ken Burns crops never upscale beyond the target."""
    w, h = size
    return ImageOps.fit(im, (w * 2, h * 2), method=Image.Resampling.LANCZOS)


def zoom_window(base, zoom, size):
    """Centered crop of the 2x base at the given zoom, resized to target."""
    w, h = size
    bw, bh = base.size
    cw, ch = bw / zoom, bh / zoom
    left = round((bw - cw) / 2)
    top = round((bh - ch) / 2)
    box = (left, top, left + round(cw), top + round(ch))
    return base.resize(size, Image.Resampling.LANCZOS, box=box)


def hold_frames(im, size, mode, count, zoom, direction, bg):
    if mode != "kenburns":
        if mode == "sequence" or mode == "crossfade":
            cover = ImageOps.fit(im, size, method=Image.Resampling.LANCZOS)
            return [cover] * count
        raise gifkit.GifError("unknown mode: %s" % mode)
    base = cover_base(im, size)
    if count <= 1:
        return [zoom_window(base, 1.0, size)]
    frames = []
    for i in range(count):
        t = i / (count - 1)
        if direction < 0:
            t = 1.0 - t
        z = 1.0 + (zoom - 1.0) * t
        frames.append(zoom_window(base, z, size))
    return frames


def build_frames(images, size, args):
    fps = args.fps
    hold_count = max(1, round(args.hold * fps))
    trans_count = 0 if args.mode == "sequence" else max(0, round(args.transition * fps))
    seqs = []
    for i, im in enumerate(images):
        direction = -1 if (args.mode == "kenburns" and i % 2) else 1
        seqs.append(hold_frames(im, size, args.mode, hold_count,
                                args.zoom, direction, args.bg))
    frames = []
    for i, seq in enumerate(seqs):
        frames.extend(seq)
        if trans_count and len(images) > 1:
            start = seqs[(i + 1) % len(seqs)][0]
            for k in range(trans_count):
                alpha = (k + 1) / (trans_count + 1)
                frames.append(Image.blend(seq[-1], start, alpha))
    return frames


def main(argv=None):
    p = argparse.ArgumentParser(
        description="Turn a folder of images into an animated GIF.")
    p.add_argument("folder", help="folder of images, sorted by name")
    p.add_argument("-o", "--out", default="slideshow.gif", help="output GIF path")
    p.add_argument("--width", type=int, default=800, help="canvas width in px")
    p.add_argument("--height", type=int, help="canvas height, default from first image")
    p.add_argument("--mode", default="crossfade",
                   choices=("sequence", "crossfade", "kenburns"))
    p.add_argument("--fps", type=int, default=12, help="frames per second, max 50")
    p.add_argument("--hold", type=float, default=1.2, help="seconds per image")
    p.add_argument("--transition", type=float, default=0.5,
                   help="crossfade seconds, 0 disables")
    p.add_argument("--zoom", type=float, default=1.10,
                   help="kenburns end zoom, for example 1.10")
    p.add_argument("--fit", default="cover", choices=("cover", "contain"),
                   help="cover crops, contain pads with --bg")
    p.add_argument("--bg", default="#000000", help="pad color for contain")
    p.add_argument("--colors", type=int, default=255, help="palette size, max 255")
    p.add_argument("--dither", default="none", choices=sorted(gifkit.DITHERS))
    p.add_argument("--loop", type=int, default=0, help="0 loops forever")
    p.add_argument("--preview", help="optional PNG of the first frame")
    args = p.parse_args(argv)

    try:
        if not 1 <= args.fps <= MAX_FPS:
            raise gifkit.GifError("fps must be between 1 and %d" % MAX_FPS)
        if args.width < 8:
            raise gifkit.GifError("width must be at least 8")
        if args.hold <= 0:
            raise gifkit.GifError("hold must be greater than 0")
        if args.zoom < 1.0:
            raise gifkit.GifError("zoom must be 1.0 or more")
        bg = gifkit.parse_color(args.bg, (0, 0, 0))
        folder = pathlib.Path(args.folder)
        if not folder.is_dir():
            raise gifkit.GifError("folder not found: %s" % folder)
        out = pathlib.Path(args.out)
        images = load_images(folder, out.resolve())
        height = target_height(args.width, images[0], args.height)
        size = (args.width, height)
        if args.mode == "kenburns":
            images = [ImageOps.pad(im, size, method=Image.Resampling.LANCZOS, color=bg)
                      if args.fit == "contain" else im
                      for im in images]
        else:
            images = [ImageOps.fit(im, size, method=Image.Resampling.LANCZOS)
                      if args.fit == "cover" else
                      ImageOps.pad(im, size, method=Image.Resampling.LANCZOS, color=bg)
                      for im in images]
        frames = build_frames(images, size, args)
        delays = gifkit.uniform_durations(len(frames), args.fps)
        result = gifkit.write_gif(frames, out, delays, loop=args.loop,
                                  colors=args.colors, dither=args.dither)
        if args.preview:
            pathlib.Path(args.preview).parent.mkdir(parents=True, exist_ok=True)
            frames[0].save(args.preview)
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
