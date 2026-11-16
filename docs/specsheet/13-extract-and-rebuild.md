# 13-extract-and-rebuild

Status: implemented. The extract route reads a GIF or APNG back into numbered PNG frames and a timing manifest. The images route reads that manifest with `--timing` and rebuilds the same timeline. Extraction writes no animation, so part 12 does not apply to its output folder.

## 13.1 The route

`anikit extract <input> -o <folder>` writes every frame of an animation as a PNG and records the source timing beside them. The route reads, it never renders: no palette is built, no delay is snapped, and the frames are written as they are decoded.

- The input format is chosen by the extension. `.gif` is read as a GIF87a or GIF89a animation, `.png` and `.apng` as an animated PNG. Any other ending fails before the file is opened.
- The input must exist and be a file. A GIF or PNG that is not an animation fails: a PNG without an `acTL` chunk is a still, not an APNG.
- The header is checked before anything is allocated: a canvas side above 8192 px or an `acTL` frame count above 100000 fails with the value in the message, before a frame buffer or the delays vector is created.
- `-o` names a folder, not a file. It defaults to `frames`. The folder must not exist yet; its parent is created when missing.
- Frames are `frame-0001.png`, `frame-0002.png` and so on, numbered from one and zero padded to four digits. A five-digit count keeps five digits, and the natural sort of the folder still follows playback order.
- Every PNG is full canvas and 8-bit RGB. Transparent source pixels are matted onto `--bg`, which defaults to `#000000`, so the frames are opaque like every other frame in the pipeline (09, row 10).
- No option other than `--bg` applies. `--colors`, `--dither`, `--loop`, `--preview` and `--font` are not options of this route, and a frame format is not selected because the route writes no animation.
- The summary line is `<folder>  N frames, X.XX s, W x H`, the same shape as the other routes, with the folder name in place of the file name.
- The whole folder is assembled under `<folder>.tmp`, verified, and renamed. A failed run removes the temp folder and leaves neither the target folder nor a partial frame.

Readback is part of the write, not a separate command. Each PNG is decoded again as it is written and compared with the frame that produced it, and the manifest is compared byte for byte, so a corrupt file or a mismatched count stops the rename.

## 13.2 Reading a GIF

The GIF reader is the `image` crate's decoder, which the images route already carries (09, row 18). It composites every frame onto the logical screen, honours local palettes, sub-rectangles, transparency and all four disposal methods, and yields one full-canvas RGBA frame per stored frame.

- A frame's delay is the graphic control extension value in 10 ms units, unmodified. The extract manifest records it in milliseconds, so a GIF delay of 0 is recorded as 0.
- The loop count is the NETSCAPE application extension. Value 0 means forever and is recorded as 0. A count `n` is recorded as `n`. A GIF with no extension is recorded as 0, because the decoders this pipeline reads treat an absent count as infinite.
- The colours are the decoded palette entries, not the source indices. Quantised colours therefore survive extraction exactly as the file stores them.
- An animated source that the `image` decoder refuses fails the run with one line; no frame folder is left.

## 13.3 Reading an APNG

The APNG reader is the `png` crate's frame decoder. The crate reads one subframe at a time and leaves the composition to the caller, because APNG blends and disposes sub-rectangles rather than whole canvases.

- The reader outputs 8-bit samples: palette entries are expanded and 16-bit samples are stripped to their high byte, giving grayscale, RGB or RGBA. Interlaced frames are deinterlaced by the crate.
- The canvas starts transparent. Every subframe is drawn into its rectangle at the `fcTL` offset. Blend `source` replaces the rectangle; blend `over` alpha-composites the subframe over the canvas.
- Disposal `none` keeps the canvas, `background` clears the frame rectangle to transparent, and `previous` restores the rectangle to its state before the frame. Disposal is applied before the next frame is drawn.
- A default image that is not part of the animation, meaning an `IDAT` without a preceding `fcTL`, is decoded and discarded, so it never becomes an extracted frame.
- A frame's delay is `delay_num / delay_den` seconds, rounded to the nearest millisecond, where a denominator of 0 counts as 100 as the APNG specification says.
- The loop count is `num_plays` from the `acTL` chunk, where 0 means forever.
- The `acTL` count must match the frames the file carries. A truncated frame, a bad chunk checksum or an offset that leaves the canvas fails the run and leaves no folder.

## 13.4 The manifest

`manifest.json` sits beside the frames and is written last. It has a fixed field order, no paths and no source name, so the folder can be moved or committed:

```json
{
  "version": 1,
  "format": "gif",
  "width": 480,
  "height": 270,
  "loop": 0,
  "delays": [100, 100, 250]
}
```

| field | type | rule |
|---|---|---|
| `version` | integer | must be 1 |
| `format` | string | `gif` or `apng`, the source format |
| `width`, `height` | integer | the source canvas, 1 to 65535 |
| `loop` | integer | 0 to 65535, where 0 means forever |
| `delays` | array of integers | one per frame in playback order, non-negative, at least one |

Delays are the presentation times of the source, in milliseconds. They are not snapped to the 10 ms grid and not merged: the extracted folder has one file per source frame, even when two frames are identical.

The reader accepts the fields in any order, rejects an unknown field, a missing field, a wrong type or an out-of-range value, and names the field in the report. The frame for entry `n` is `frame-<n>.png` with the four-digit padding of 13.1; a folder whose frames do not match the delay count fails the rebuild.

## 13.5 Rebuilding the timing

`anikit images <folder> -o <file> --timing <manifest>` writes an animation from the extracted frames with the recorded timing. The route still owns fitting and the writer, and the output extension still chooses GIF or APNG.

- Every still is shown once, in natural folder order, for its delay from the manifest. Presentation times are cumulative milliseconds, so the timeline matches the source up to the writer's own grid: GIF and APNG snap on the cumulative clock to 10 ms and keep delays at 20 ms or more, as every route does.
- `--mode`, `--fps`, `--hold`, `--transition` and `--zoom` do not apply and are documented as ignored. There is no frame grid to resample and no blend to insert.
- `--width` and `--height` default to the manifest's canvas, per axis: a missing width takes the manifest width, a missing height the manifest height. `--fit`, `--bg`, `--colors` and `--dither` apply as on any images run.
- `--loop` sets the play count as usual. When `--loop` is absent, the manifest loop count is used.
- The image count must equal the delay count. A mismatch fails before any frame is rendered, names both counts and leaves no output.
- Identical consecutive stills merge into one stored frame with the summed delay, as on every route, so a rebuilt file can hold fewer frames than the folder while the timeline stays the same.

A round trip through this pair preserves frame count, per-frame delays, total duration and canvas size when the source delays are multiples of 10 ms and no two consecutive frames are identical. Otherwise the timeline is preserved and the merge or the 20 ms floor applies, exactly as the format requires.

An APNG that this pipeline wrote comes back byte for byte, because APNG is truecolor and rebuilds no palette. A GIF rebuild samples a new palette under the rule of part 03, so a frame can shift by a few levels where the source used colours that its twelve samples do not carry.

## 13.6 Errors

The extract route reports, with one `FAIL` line, exit 1 and no folder:

- the input is missing, or its extension is not `.gif`, `.png` or `.apng`;
- the input cannot be read or decoded, or is not an animation;
- the output folder or its temp folder already exists;
- a frame or the manifest fails its readback.

The images route with `--timing` reports, with one `FAIL` line and no output:

- the manifest is missing, not JSON, has an unknown field or a bad value;
- the delay count differs from the number of stills in the folder;
- a still cannot be decoded.

## 13.7 Tests

- `anikit-render-raster/src/extract.rs` writes a two-frame APNG with the core writer, extracts it, and checks the frames, delays, loop count, size and manifest text. A second test builds a GIF with a transparent pixel and a smaller frame rectangle through the `gif` dev-dependency, extracts it with a matte, and checks the composited pixels.
- The same module checks that an existing folder, an unknown extension and a still PNG are refused and leave no folder.
- `anikit-render-raster/src/slideshow.rs` rebuilds two stills from an in-memory timing and checks frame count, per-frame delays and total on the written GIF.
- `anikit-cli` runs a GIF through `ascii`, extracts it, rebuilds it with `images --timing`, and compares frame count, delays, total and size. The APNG round trip is checked the same way.

## 13.8 Limits

- Extraction writes one PNG per stored frame. A heavily merged GIF therefore expands to its stored count, not its presentation count.
- A source delay that is not a multiple of 10 ms, or below 20 ms, cannot be reproduced exactly by GIF or APNG; the writer's grid applies on rebuild.
- The manifest has no source name and no absolute path. It describes the folder, not where the folder came from.
- Extract and rebuild read and write whole files on disk. They take no part in the streaming pipeline of part 03 beyond writing one decoded frame at a time.
