//! Structural comparison against the frozen Python images fixture.
//!
//! Part 08 records frame count, delays, size and pixel hashes. The route is
//! compared on everything except the hashes: the port resamples with its own
//! Lanczos pass and palette, so pixels differ by design (09, row 18).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anikit_core::inspect_file;

#[path = "../../../testsupport/decode.rs"]
mod decode;

const BIN: &str = env!("CARGO_BIN_EXE_anikit");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/images")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn manifest() -> (HashMap<String, String>, Vec<(u32, String)>) {
    let text = std::fs::read_to_string(fixture().join("manifest.txt")).expect("manifest");
    let mut keys = HashMap::new();
    let mut frames = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("frame ") {
            let mut delay = 0u32;
            let mut hash = String::new();
            for field in rest.split_whitespace().skip(1) {
                if let Some(value) = field.strip_prefix("delay_ms=") {
                    delay = value.parse().expect("frame delay");
                } else if let Some(value) = field.strip_prefix("hash=") {
                    hash = value.to_string();
                }
            }
            frames.push((delay, hash));
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

#[test]
fn images_fixture_matches_the_recorded_structure() {
    let dir = scratch("images-fixture");
    let out = dir.join("out.gif");
    let input = fixture().join("input");
    let output = run(&[
        "images",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--width",
        "320",
        "--height",
        "180",
        "--mode",
        "crossfade",
        "--fps",
        "12",
        "--hold",
        "0.5",
        "--transition",
        "0.25",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("12 frames"));

    let (keys, frames) = manifest();
    assert_eq!(frames.len(), keys["frame_count"].parse::<usize>().unwrap());

    let info = inspect_file(&out).unwrap();
    assert_eq!(info.frames.to_string(), keys["frame_count"]);
    assert_eq!(info.total_ms.to_string(), keys["total_ms"]);
    assert_eq!(info.width.to_string(), keys["width"]);
    assert_eq!(info.height.to_string(), keys["height"]);

    let (width, height, decoded) = decode::canvases(&out);
    assert_eq!((width as u32, height as u32), (320, 180), "canvas");
    assert_eq!(decoded.len(), frames.len());
    for (index, (buffer, delay)) in decoded.iter().enumerate() {
        assert_eq!(*delay, frames[index].0, "frame {index} delay");
        assert!(!buffer.is_empty());
    }
    assert_ne!(decoded[0].0, decoded[1].0, "frames must differ");
    assert!(!dir.join("out.tmp.gif").exists());

    // The third image carries EXIF orientation 6, so its hold shows the red
    // half on top and the blue half at the bottom. Frame 8 is that hold.
    let hold = &decoded[8].0;
    let at = |x: usize, y: usize| {
        let i = (y * 320 + x) * 4;
        [hold[i], hold[i + 1], hold[i + 2]]
    };
    let top = at(160, 30);
    let bottom = at(160, 150);
    assert!(top[0] > 150 && top[2] < 100, "top {top:?}");
    assert!(bottom[2] > 150 && bottom[0] < 100, "bottom {bottom:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_folder_reports_no_images() {
    let dir = scratch("images-empty");
    std::fs::write(dir.join("notes.txt"), b"not an image").unwrap();
    let output = run(&[
        "images",
        dir.to_str().unwrap(),
        "-o",
        dir.join("out.gif").to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no images"), "stderr: {stderr}");
    assert!(!dir.join("out.gif").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
