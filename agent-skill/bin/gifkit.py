"""Shared GIF assembly for the animated-gif tools.

Frames arrive as RGB images. One palette is built from frames sampled across
the loop, every frame is quantized onto it, and the result is written
atomically and verified: frame count, duration sum, dimensions. On a mismatch
the temp file is removed and GifError is raised, so a printed path is always a
complete GIF.
"""
import os
import pathlib
from collections import namedtuple

from PIL import Image

MIN_DELAY_MS = 20
MAX_COLORS = 255
DITHERS = {
    "none": Image.Dither.NONE,
    "floyd": Image.Dither.FLOYDSTEINBERG,
}

GifResult = namedtuple("GifResult", "path frames total_ms size")


def merge_runs(frames, durations):
    """Collapse identical consecutive frames into one frame with a longer delay."""
    if not frames:
        return frames, durations
    out_frames = [frames[0]]
    out_durations = [durations[0]]
    last = frames[0].tobytes()
    for im, ms in zip(frames[1:], durations[1:]):
        raw = im.tobytes()
        if raw == last:
            out_durations[-1] += ms
        else:
            out_frames.append(im)
            out_durations.append(ms)
            last = raw
    return out_frames, out_durations


class GifError(Exception):
    """Raised when the written GIF does not match the requested frames."""


def scaled_durations(runs, speed=1.0):
    """Frame durations in ms at a speed factor, on the 10 ms GIF grid.

    Rounding is done on cumulative time and then differenced, so error does
    not accumulate over the loop. Runs are (state, ms) pairs or plain ms
    values.
    """
    out = []
    elapsed = 0
    emitted = 0
    for run in runs:
        ms = run[1] if isinstance(run, (tuple, list)) else run
        elapsed += ms
        target = round(elapsed / speed / 10) * 10
        out.append(max(MIN_DELAY_MS, target - emitted))
        emitted += out[-1]
    return out


def uniform_durations(count, fps):
    """Count frames at fps, snapped to the 10 ms unit and at least 20 ms."""
    if fps <= 0:
        raise GifError("fps must be greater than 0")
    delay = max(MIN_DELAY_MS, round(1000.0 / fps / 10) * 10)
    return [delay] * count


def flatten(im, background=(0, 0, 0)):
    """Convert any Pillow image to RGB over a solid background."""
    if im.mode == "RGB":
        return im
    if im.mode in ("RGBA", "LA") or (im.mode == "P" and "transparency" in im.info):
        im = im.convert("RGBA")
        base = Image.new("RGB", im.size, background)
        base.paste(im, mask=im.getchannel("A"))
        return base
    return im.convert("RGB")


def sample_palette(frames, colors=MAX_COLORS, picks=12):
    """One palette for every frame, so colors cannot flicker between frames.

    Frames are sampled evenly across the loop and stacked before quantization.
    """
    if not frames:
        raise GifError("no frames to sample")
    count = len(frames)
    if count <= picks:
        chosen = list(frames)
    else:
        chosen = [frames[round(i * (count - 1) / (picks - 1))] for i in range(picks)]
    w, h = chosen[0].size
    strip = Image.new("RGB", (w, h * len(chosen)))
    for k, im in enumerate(chosen):
        strip.paste(im, (0, k * h))
    return strip.quantize(colors=colors, method=Image.Quantize.MEDIANCUT)


def write_gif(frames, path, durations, loop=0, colors=MAX_COLORS, dither="none"):
    """Quantize frames onto one palette, write atomically, verify, return path.

    frames: RGB images, all the same size.
    durations: per-frame delays in ms, multiples of 10.
    """
    if not frames:
        raise GifError("no frames to write")
    if not 2 <= colors <= MAX_COLORS:
        raise GifError("colors must be between 2 and %d" % MAX_COLORS)
    if len(frames) != len(durations):
        raise GifError("frame count and duration count differ")
    if dither not in DITHERS:
        raise GifError("dither must be one of: %s" % ", ".join(sorted(DITHERS)))
    size = frames[0].size
    for im in frames:
        if im.size != size:
            raise GifError("frame sizes differ: %s vs %s" % (size, im.size))
        if im.mode != "RGB":
            raise GifError("frames must be RGB")
    durations = [max(MIN_DELAY_MS, int(round(ms / 10.0)) * 10) for ms in durations]
    frames, durations = merge_runs(frames, durations)

    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.stem + ".tmp" + path.suffix)

    palette = sample_palette(frames, colors=min(colors, MAX_COLORS))
    indexed = [im.quantize(palette=palette, dither=DITHERS[dither])
               for im in frames]
    total_ms = sum(durations)
    try:
        indexed[0].save(tmp, save_all=True, append_images=indexed[1:],
                        duration=durations, loop=loop, disposal=1,
                        optimize=False)
        verify_gif(tmp, len(frames), total_ms, size)
        os.replace(tmp, path)
    except Exception:
        if tmp.exists():
            try:
                tmp.unlink()
            except OSError:
                pass
        raise
    return GifResult(path, len(frames), total_ms, size)


def verify_gif(path, frames, total_ms, size):
    """Read the file back and compare frame count, duration sum and size."""
    path = pathlib.Path(path)
    if not path.is_file():
        raise GifError("file was not written: %s" % path)
    im = Image.open(path)
    count = 0
    total = 0
    try:
        while True:
            count += 1
            total += int(im.info.get("duration", 0) or 0)
            try:
                im.seek(im.tell() + 1)
            except EOFError:
                break
        actual_size = im.size
    finally:
        im.close()
    if actual_size != tuple(size):
        raise GifError("size %s does not match %s" % (actual_size, tuple(size)))
    if count != frames:
        raise GifError("frame count %d does not match %d" % (count, frames))
    if abs(total - total_ms) > 10:
        raise GifError("duration %d ms does not match %d ms" % (total, total_ms))


def parse_color(text, fallback=(255, 255, 255)):
    """Parse #rgb, #rrggbb or r,g,b. Returns an RGB tuple."""
    if not text:
        return fallback
    s = text.strip().lstrip("#")
    if "," in s:
        parts = [int(p) for p in s.split(",")]
        if len(parts) != 3 or not all(0 <= p <= 255 for p in parts):
            raise GifError("invalid color: %s" % text)
        return tuple(parts)
    if len(s) == 3:
        s = "".join(c * 2 for c in s)
    if len(s) != 6:
        raise GifError("invalid color: %s" % text)
    try:
        return tuple(int(s[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        raise GifError("invalid color: %s" % text)
