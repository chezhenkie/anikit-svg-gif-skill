//! Round-trip check with an independent decoder.
//!
//! The writer's own readback verifies structure; this test proves the bytes are
//! a GIF that another implementation can decode back to the same pixels.

use std::path::{Path, PathBuf};

use anikit_core::{Dither, Frame, FrameSink, OutputFormat, StreamOptions, stream_output};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn solid(rgb: [u8; 3]) -> Frame {
    let mut frame = Frame::new(8, 8).unwrap();
    frame.fill(rgb);
    frame
}

/// Stream the frames at even spacing across a loop of `total_ms`.
fn write(path: &Path, frames: &[Frame], colors: usize, dither: Dither, total_ms: u32) {
    let (width, height) = frames[0].size();
    let options = StreamOptions {
        format: OutputFormat::Gif,
        width,
        height,
        loop_count: 0,
        colors,
        dither,
        total_ms,
    };
    let mut stream = stream_output(path, frames, options).unwrap();
    let step = total_ms as u64 * 1000 / frames.len() as u64;
    for (index, frame) in frames.iter().enumerate() {
        stream
            .push(index as u32, index as u64 * step, frame)
            .unwrap();
    }
    stream.finish().unwrap();
}

#[test]
fn written_gif_decodes_to_the_same_pixels() {
    let dir = scratch("roundtrip");
    let path = dir.join("two.gif");
    let frames = vec![solid([255, 0, 0]), solid([0, 0, 255])];
    write(&path, &frames, 255, Dither::None, 200);

    let bytes = std::fs::read(&path).unwrap();
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(&bytes[..]).unwrap();

    let mut decoded: Vec<(Vec<u8>, u16)> = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        decoded.push((frame.buffer.to_vec(), frame.delay));
    }

    assert_eq!(decoded.len(), 2, "frame count");
    assert_eq!(decoded[0].1, 10, "first delay in centiseconds");
    assert_eq!(decoded[1].1, 10, "second delay in centiseconds");
    for (rgba, _) in &decoded {
        assert_eq!(rgba.len(), 8 * 8 * 4);
    }
    assert_eq!(&decoded[0].0[0..4], &[255, 0, 0, 255], "first pixel");
    assert_eq!(&decoded[1].0[0..4], &[0, 0, 255, 255], "second frame pixel");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dithered_gradient_still_decodes() {
    let dir = scratch("roundtrip-dither");
    let path = dir.join("gradient.gif");
    let mut frame = Frame::new(32, 8).unwrap();
    for x in 0..32 {
        let v = (x * 8) as u8;
        for y in 0..8 {
            frame.set_pixel(x, y, [v, v, v]);
        }
    }
    write(&path, &[frame], 8, Dither::Floyd, 100);

    let bytes = std::fs::read(&path).unwrap();
    let mut decode = gif::DecodeOptions::new();
    decode.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = decode.read_info(&bytes[..]).unwrap();
    let frame = decoder.read_next_frame().unwrap().expect("one frame");
    assert_eq!(frame.width, 32);
    assert_eq!(frame.height, 8);
    let _ = std::fs::remove_dir_all(&dir);
}
