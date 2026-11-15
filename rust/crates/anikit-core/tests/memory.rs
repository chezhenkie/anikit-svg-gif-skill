//! Peak memory against frame count (part 03.4, part 08.3).
//!
//! The stream holds one frame, so the peak should not move when the loop grows
//! from 100 frames to 1000. The check reads the high-water mark from the
//! operating system rather than counting allocations, because allocator
//! retention is part of what the limit is about.

use std::path::Path;

use anikit_core::{Dither, Frame, FrameSink, StreamOptions, stream_gif};

const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;

#[test]
fn peak_memory_does_not_grow_with_frame_count() {
    if peak_rss_kb().is_none() {
        eprintln!("skipping: peak resident set is not readable on this target");
        return;
    }
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("memory");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");

    write_run(&dir, "hundred.gif", 100);
    let small = peak_rss_kb().unwrap();
    write_run(&dir, "thousand.gif", 1000);
    let large = peak_rss_kb().unwrap();

    eprintln!("peak resident set: 100 frames {small} KiB, 1000 frames {large} KiB");
    let growth = large.saturating_sub(small);
    assert!(
        growth < 2048,
        "peak grew by {growth} KiB when the loop went from 100 to 1000 frames"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn write_run(dir: &Path, name: &str, count: u32) {
    let samples: Vec<Frame> = (0..4).map(|i| frame_at(i * count / 4)).collect();
    let options = StreamOptions {
        loop_count: 0,
        colors: 255,
        dither: Dither::None,
        total_ms: count * 40,
    };
    let mut stream = stream_gif(&dir.join(name), &samples, options).expect("open stream");
    for index in 0..count {
        let frame = frame_at(index);
        stream
            .push(index, index as u64 * 40_000, &frame)
            .expect("push frame");
    }
    let summary = stream.finish().expect("finish stream");
    assert_eq!(summary.frames, count);
}

/// A shaded background with one bar that moves each frame.
fn frame_at(index: u32) -> Frame {
    let mut frame = Frame::new(WIDTH, HEIGHT).expect("frame");
    for y in 0..HEIGHT {
        let shade = (y * 255 / HEIGHT) as u8;
        for x in 0..WIDTH {
            frame.set_pixel(x, y, [shade, shade / 2, 255 - shade]);
        }
    }
    let left = (index * 7) % (WIDTH - 32);
    for y in 20..80 {
        for x in left..left + 32 {
            frame.set_pixel(x, y, [255, 255, 255]);
        }
    }
    frame
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

    // SAFETY: the call only fills the structure passed to it, and its size is
    // the size of the structure the API expects.
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
