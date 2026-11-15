//! Round-trip check with an independent decoder.
//!
//! The writer's own readback verifies structure; this test proves the bytes are
//! a GIF that another implementation can decode back to the same pixels.

use std::path::PathBuf;

use anikit_core::{Frame, WriteOptions, write_gif};

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

#[test]
fn written_gif_decodes_to_the_same_pixels() {
    let dir = scratch("roundtrip");
    let path = dir.join("two.gif");
    let frames = vec![solid([255, 0, 0]), solid([0, 0, 255])];
    write_gif(&frames, &[100, 100], &path, &WriteOptions::default()).unwrap();

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
    let options = WriteOptions {
        colors: 8,
        dither: anikit_core::Dither::Floyd,
        ..Default::default()
    };
    write_gif(&[frame], &[100], &path, &options).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let mut decode = gif::DecodeOptions::new();
    decode.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = decode.read_info(&bytes[..]).unwrap();
    let frame = decoder.read_next_frame().unwrap().expect("one frame");
    assert_eq!(frame.width, 32);
    assert_eq!(frame.height, 8);
    let _ = std::fs::remove_dir_all(&dir);
}
