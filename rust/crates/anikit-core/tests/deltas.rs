//! Delta rectangles, decoded back with an independent decoder.
//!
//! Part 03.3 makes every frame after the first a rectangle over the previous
//! canvas. These tests composite the patches the way a viewer does and compare
//! the result against the pixels that went in, so a wrong rectangle or a stale
//! canvas cannot pass.

use std::fs::File;
use std::path::PathBuf;

use anikit_core::{
    Dither, Frame, FrameSink, GifEncoder, OutputFormat, Palette, StreamOptions, stream_output,
};

#[path = "../../../testsupport/decode.rs"]
mod decode;

const WIDTH: usize = 24;
const HEIGHT: usize = 16;

const BLACK: [u8; 3] = [0, 0, 0];
const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const WHITE: [u8; 3] = [255, 255, 255];

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn palette() -> Palette {
    Palette::from_colors(vec![BLACK, RED, BLUE, WHITE]).unwrap()
}

fn rgba(index: u8) -> [u8; 4] {
    match index {
        1 => [RED[0], RED[1], RED[2], 255],
        2 => [BLUE[0], BLUE[1], BLUE[2], 255],
        3 => [WHITE[0], WHITE[1], WHITE[2], 255],
        _ => [0, 0, 0, 255],
    }
}

fn blank() -> Vec<u8> {
    vec![0; WIDTH * HEIGHT]
}

fn block(canvas: &mut [u8], x: usize, y: usize, index: u8) {
    for row in y..y + 4 {
        for col in x..x + 4 {
            canvas[row * WIDTH + col] = index;
        }
    }
}

/// A moving block and one changed pixel, as palette indices.
fn frames() -> Vec<Vec<u8>> {
    let mut canvas = blank();
    block(&mut canvas, 2, 2, 1);
    let mut frames = vec![canvas.clone()];
    block(&mut canvas, 2, 2, 0);
    block(&mut canvas, 3, 2, 1);
    frames.push(canvas.clone());
    block(&mut canvas, 3, 2, 0);
    block(&mut canvas, 3, 5, 2);
    frames.push(canvas.clone());
    canvas[14 * WIDTH + 22] = 3;
    frames.push(canvas.clone());
    frames
}

fn encode(path: &PathBuf, frames: &[Vec<u8>]) {
    let file = File::create(path).expect("create gif");
    let mut encoder =
        GifEncoder::new(file, WIDTH as u32, HEIGHT as u32, &palette(), 0).expect("header");
    for frame in frames {
        encoder.write_frame(frame, 100).expect("frame");
    }
    let summary = encoder.finish().expect("trailer");
    assert_eq!(summary.frames as usize, frames.len());
}

#[test]
fn a_delta_stream_composites_to_the_input_pixels() {
    let dir = scratch("deltas-composite");
    let path = dir.join("move.gif");
    let frames = frames();
    encode(&path, &frames);

    let (width, height, decoded) = decode::canvases(&path);
    assert_eq!((width as usize, height as usize), (WIDTH, HEIGHT));
    assert_eq!(decoded.len(), frames.len());
    for (index, (pixels, delay)) in decoded.iter().enumerate() {
        assert_eq!(*delay, 100, "frame {index} delay");
        let want: Vec<u8> = frames[index].iter().flat_map(|i| rgba(*i)).collect();
        assert_eq!(pixels, &want, "frame {index} pixels");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn each_frame_draws_only_what_changed() {
    let dir = scratch("deltas-patches");
    let path = dir.join("move.gif");
    encode(&path, &frames());

    let (_, _, patches) = decode::patches(&path);
    let rect = |patch: &decode::Patch| (patch.left, patch.top, patch.width, patch.height);
    assert_eq!(rect(&patches[0]), (0, 0, WIDTH as u16, HEIGHT as u16));
    assert_eq!(rect(&patches[1]), (2, 2, 5, 4));
    assert_eq!(rect(&patches[2]), (3, 2, 4, 7));
    assert_eq!(rect(&patches[3]), (22, 14, 1, 1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_stream_route_writes_deltas_too() {
    let dir = scratch("deltas-stream");
    let path = dir.join("stream.gif");
    let frames: Vec<Frame> = frames()
        .iter()
        .map(|indices| {
            let mut frame = Frame::new(WIDTH as u32, HEIGHT as u32).unwrap();
            for (index, byte) in indices.iter().enumerate() {
                frame.set_pixel(
                    (index % WIDTH) as u32,
                    (index / WIDTH) as u32,
                    match *byte {
                        1 => RED,
                        2 => BLUE,
                        3 => WHITE,
                        _ => BLACK,
                    },
                );
            }
            frame
        })
        .collect();
    let options = StreamOptions {
        format: OutputFormat::Gif,
        width: WIDTH as u32,
        height: HEIGHT as u32,
        loop_count: 0,
        colors: 255,
        dither: Dither::None,
        total_ms: 400,
    };
    let mut stream = stream_output(&path, &frames, options).unwrap();
    for (index, frame) in frames.iter().enumerate() {
        stream
            .push(index as u32, index as u64 * 100_000, frame)
            .unwrap();
    }
    stream.finish().unwrap();

    let (_, _, patches) = decode::patches(&path);
    assert_eq!(patches.len(), frames.len(), "frame count");
    let small = patches
        .iter()
        .any(|patch| patch.width < WIDTH as u16 || patch.height < HEIGHT as u16);
    assert!(small, "no frame was written as a rectangle");
    let _ = std::fs::remove_dir_all(&dir);
}
