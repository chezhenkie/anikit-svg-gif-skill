//! Structural comparison against the frozen Python vector fixture.
//!
//! Part 08 records frame count, delays, size and pixel hashes. The reference was
//! captured in Chromium 146 through Playwright; the port reproduces those exact
//! pixels in later builds, so the hashes are compared as well as the structure.
//! The block positions make a failure readable: they separate a missed seek
//! from a moved palette.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anikit_core::inspect_file;
use sha2::{Digest, Sha256};

#[path = "../../../testsupport/decode.rs"]
mod decode;

#[path = "../../../testsupport/browser.rs"]
mod browser_support;

const BIN: &str = env!("CARGO_BIN_EXE_anikit");
const WIDTH: u32 = 120;

/// Tests that start a browser take this lock, one browser at a time.
static BROWSER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vector")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// One recorded frame: its delay, where the two blocks sat, and its pixels.
#[derive(Debug)]
struct Recorded {
    delay: u32,
    red_x: f64,
    blue_x: f64,
    hash: String,
}

fn manifest() -> (HashMap<String, String>, Vec<Recorded>) {
    let text = std::fs::read_to_string(fixture().join("manifest.txt")).expect("manifest");
    let mut keys = HashMap::new();
    let mut frames = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("frame ") {
            let mut delay = 0u32;
            let mut red_x = 0.0;
            let mut blue_x = 0.0;
            let mut hash = String::new();
            for field in rest.split_whitespace().skip(1) {
                let (name, value) = field.split_once('=').expect("frame field");
                match name {
                    "delay_ms" => delay = value.parse().expect("frame delay"),
                    "red_x" => red_x = value.parse().expect("red position"),
                    "blue_x" => blue_x = value.parse().expect("blue position"),
                    "hash" => hash = value.to_string(),
                    _ => {}
                }
            }
            frames.push(Recorded {
                delay,
                red_x,
                blue_x,
                hash,
            });
        } else if let Some((key, value)) = line.split_once('=') {
            keys.insert(key.to_string(), value.to_string());
        }
    }
    (keys, frames)
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("run the anikit binary")
}

fn decoded(path: &Path) -> Vec<(Vec<u8>, u32)> {
    let (_, _, frames) = decode::canvases(path);
    frames
}

/// The mean x of pixels close to one colour.
fn centre(frame: &[u8], want: [u8; 3]) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for (index, pixel) in frame.as_chunks::<4>().0.iter().enumerate() {
        let close = (0..3).all(|channel| (pixel[channel] as i32 - want[channel] as i32).abs() < 48);
        if close {
            sum += (index as u32 % WIDTH) as f64;
            count += 1.0;
        }
    }
    assert!(count > 0.0, "no pixels of {want:?} in the frame");
    sum / count
}

/// The hash of a decoded frame's RGB bytes.
fn frame_hash(rgba: &[u8]) -> String {
    let mut hasher = Sha256::new();
    for pixel in rgba.as_chunks::<4>().0 {
        hasher.update(&pixel[..3]);
    }
    format!("{:x}", hasher.finalize())
}

#[test]
fn vector_fixture_matches_the_recorded_structure() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    // The capture needs a browser; without one there is nothing to compare.
    if browser_support::browser().is_none() {
        return;
    }
    let dir = scratch("vector-fixture");
    let out = dir.join("out.gif");
    let input = fixture().join("input/animation.svg");
    let output = run(&[
        "vector",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--duration",
        "1",
        "--fps",
        "10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("10 frames"));

    let (keys, frames) = manifest();
    assert_eq!(frames.len(), keys["frame_count"].parse::<usize>().unwrap());

    let info = inspect_file(&out).unwrap();
    assert_eq!(info.frames.to_string(), keys["frame_count"]);
    assert_eq!(info.total_ms.to_string(), keys["total_ms"]);
    assert_eq!(info.width.to_string(), keys["width"]);
    assert_eq!(info.height.to_string(), keys["height"]);

    let captured = decoded(&out);
    assert_eq!(captured.len(), frames.len());
    for (index, (buffer, delay)) in captured.iter().enumerate() {
        assert_eq!(*delay, frames[index].delay, "frame {index} delay");
        assert_eq!(
            frame_hash(buffer),
            frames[index].hash,
            "frame {index} pixels"
        );
        let red = centre(buffer, [0xd0, 0x20, 0x20]);
        let blue = centre(buffer, [0x20, 0x40, 0xd0]);
        assert!(
            (red - frames[index].red_x).abs() < 2.0,
            "frame {index} CSS block at {red}, recorded {}",
            frames[index].red_x
        );
        assert!(
            (blue - frames[index].blue_x).abs() < 2.0,
            "frame {index} SMIL block at {blue}, recorded {}",
            frames[index].blue_x
        );
    }
    // The CSS block moves right and the SMIL block moves left, so a clock that
    // was never seeked cannot pass both ends of the loop.
    assert!(frames[9].red_x > frames[0].red_x + 40.0);
    assert!(frames[9].blue_x < frames[0].blue_x - 40.0);
    assert!(!dir.join("out.tmp.gif").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_device_pixel_ratio_doubles_the_canvas() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if browser_support::browser().is_none() {
        return;
    }
    let dir = scratch("vector-dpr");
    let out = dir.join("out.gif");
    let input = fixture().join("input/animation.svg");
    let output = run(&[
        "vector",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--duration",
        "1",
        "--fps",
        "10",
        "--dpr",
        "2",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("240 x 160"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let info = inspect_file(&out).unwrap();
    assert_eq!((info.width, info.height), (240, 160));
    assert_eq!(info.frames, 10);
    assert_eq!(info.total_ms, 1000);
    let (width, height, _) = decode::canvases(&out);
    assert_eq!((width, height), (240, 160));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_input_reports_fail_and_writes_nothing() {
    let dir = scratch("vector-failure");
    let out = dir.join("nope.gif");
    let output = run(&[
        "vector",
        dir.join("missing.svg").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("FAIL "), "stderr: {stderr}");
    assert!(stderr.contains("input not found"), "stderr: {stderr}");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preset_without_a_target_reports_the_selector() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if browser_support::browser().is_none() {
        return;
    }
    let dir = scratch("vector-selector");
    let input = fixture().join("input/animation.svg");
    let out = dir.join("out.gif");
    let output = run(&[
        "vector",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--animate",
        "pulse",
        "--selector",
        "#not-there",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("FAIL "), "stderr: {stderr}");
    assert!(stderr.contains("selector not found"), "stderr: {stderr}");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
