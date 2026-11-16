//! Peak memory of the images route against input size (part 03.4, part 08.3).
//!
//! The route streams its sources, so the peak should follow the canvas and the
//! twelve palette samples, not the number of frames. These checks compare the
//! high-water mark of a small input with a much larger one.

use std::path::{Path, PathBuf};

use anikit_core::{Dither, Frame, FrameSink, OutputFormat, StreamOptions, stream_output};
use anikit_render_raster::{Fit, Mode, Options, write_slideshow};
use image::{Rgba, RgbaImage};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;

fn options() -> Options {
    Options {
        width: WIDTH,
        height: Some(HEIGHT),
        mode: Mode::Sequence,
        fps: 10,
        hold: 0.1,
        transition: 0.0,
        zoom: 1.0,
        fit: Fit::Cover,
        bg: [0, 0, 0],
        colors: 255,
        dither: Dither::None,
        loop_count: 0,
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn still(path: &Path, shift: u32) {
    let image = RgbaImage::from_fn(640, 360, |x, y| {
        Rgba([((x + shift) % 256) as u8, ((y * 255 / 360) as u8), 128, 255])
    });
    image.save(path).expect("write png");
}

fn animated_gif(path: &Path, count: u32) {
    let options = StreamOptions {
        format: OutputFormat::Gif,
        width: WIDTH,
        height: HEIGHT,
        loop_count: 0,
        colors: 255,
        dither: Dither::None,
        total_ms: count * 40,
    };
    let sample = bar_frame(0);
    let mut stream = stream_output(path, &[sample], options).expect("open gif");
    for index in 0..count {
        let frame = bar_frame(index);
        stream
            .push(index, index as u64 * 40_000, &frame)
            .expect("push frame");
    }
    stream.finish().expect("finish gif");
}

/// A background with one bar that moves each frame.
fn bar_frame(index: u32) -> Frame {
    let mut frame = Frame::new(WIDTH, HEIGHT).expect("frame");
    for y in 0..HEIGHT {
        let shade = (y * 255 / HEIGHT) as u8;
        for x in 0..WIDTH {
            frame.set_pixel(x, y, [shade, shade / 3, 120]);
        }
    }
    let left = (index * 11) % (WIDTH - 40);
    for y in 40..100 {
        for x in left..left + 40 {
            frame.set_pixel(x, y, [255, 255, 255]);
        }
    }
    frame
}

fn peak_run(folder: &Path, name: &str) -> u64 {
    let out = folder.with_file_name(format!("{name}.gif"));
    write_slideshow(folder, &out, &options()).expect("run the images route");
    peak_rss_kb().expect("peak resident set")
}

#[test]
fn peak_memory_does_not_grow_with_the_folder() {
    if peak_rss_kb().is_none() {
        eprintln!("skipping: peak resident set is not readable on this target");
        return;
    }
    let dir = scratch("memory-folder");
    let ten = dir.join("ten");
    let fifty = dir.join("fifty");
    std::fs::create_dir_all(&ten).unwrap();
    std::fs::create_dir_all(&fifty).unwrap();
    for index in 0..50 {
        let path = fifty.join(format!("{index:03}.png"));
        still(&path, index * 7);
        if index < 10 {
            still(&ten.join(format!("{index:03}.png")), index * 7);
        }
    }

    let small = peak_run(&ten, "ten");
    let large = peak_run(&fifty, "fifty");
    eprintln!("peak resident set: 10 stills {small} KiB, 50 stills {large} KiB");
    let growth = large.saturating_sub(small);
    assert!(
        growth < 8192,
        "peak grew by {growth} KiB from 10 to 50 stills"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn peak_memory_does_not_grow_with_an_animated_source() {
    if peak_rss_kb().is_none() {
        eprintln!("skipping: peak resident set is not readable on this target");
        return;
    }
    let dir = scratch("memory-gif");
    let small = dir.join("small");
    let large = dir.join("large");
    std::fs::create_dir_all(&small).unwrap();
    std::fs::create_dir_all(&large).unwrap();
    animated_gif(&small.join("clip.gif"), 50);
    animated_gif(&large.join("clip.gif"), 500);

    let first = peak_run(&small, "small");
    let second = peak_run(&large, "large");
    eprintln!("peak resident set: 50 frames {first} KiB, 500 frames {second} KiB");
    let growth = second.saturating_sub(first);
    assert!(
        growth < 8192,
        "peak grew by {growth} KiB from 50 to 500 source frames"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(target_os = "linux")]
fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

#[cfg(windows)]
fn peak_rss_kb() -> Option<u64> {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_non_paged: usize,
        quota_non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }

    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, size: u32) -> i32;
    }

    // SAFETY: the call only fills the structure passed to it.
    unsafe {
        let mut counters: Counters = std::mem::zeroed();
        counters.cb = std::mem::size_of::<Counters>() as u32;
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) == 0 {
            return None;
        }
        Some(counters.peak_working_set as u64 / 1024)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn peak_rss_kb() -> Option<u64> {
    None
}
