//! Structural comparison against the frozen Python fixture.
//!
//! Part 08 compares a route by frame count, delays, size and pixel hashes.
//! The text route compares everything except the hashes: the port draws the
//! same face without hinting, so its pixels differ by design (09, row 11).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anikit_core::inspect_file;

#[path = "../../../testsupport/decode.rs"]
mod decode;

const BIN: &str = env!("CARGO_BIN_EXE_anikit");

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text")
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

fn text_fixture_args<'a>(input: &'a str, out: &'a str) -> Vec<&'a str> {
    vec![
        "ascii", input, "-o", out, "--size", "28", "--width", "480", "--height", "270",
    ]
}

#[test]
fn text_fixture_matches_the_recorded_structure() {
    let dir = scratch("text-fixture");
    let out = dir.join("out.gif");
    let input = fixture().join("frames.txt");
    let args = text_fixture_args(input.to_str().unwrap(), out.to_str().unwrap());
    let output = run(&args);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("3 frames"));

    let (keys, frames) = manifest();
    assert_eq!(frames.len(), keys["frame_count"].parse::<usize>().unwrap());

    let info = inspect_file(&out).unwrap();
    assert_eq!(info.frames.to_string(), keys["frame_count"]);
    assert_eq!(info.total_ms.to_string(), keys["total_ms"]);
    assert_eq!(info.width.to_string(), keys["width"]);
    assert_eq!(info.height.to_string(), keys["height"]);

    let (width, height, decoded) = decode::canvases(&out);
    assert_eq!((width, height), (480, 270), "canvas");
    assert_eq!(decoded.len(), frames.len());
    for (index, (buffer, delay)) in decoded.iter().enumerate() {
        assert_eq!(*delay, frames[index].0, "frame {index} delay");
        assert!(!buffer.is_empty(), "frame {index} decoded empty");
    }
    assert_ne!(decoded[0].0, decoded[1].0, "frames must differ");
    assert!(!dir.join("out.tmp.gif").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_off_grid_delay_totals_on_the_grid() {
    let dir = scratch("text-cumulative");
    let input = dir.join("frames.txt");
    std::fs::write(&input, "a\n---\nb\n---\nc\n").unwrap();
    let out = dir.join("out.gif");
    let output = run(&[
        "ascii",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--size",
        "28",
        "--delay",
        "33",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("0.10 s"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let info = inspect_file(&out).unwrap();
    assert_eq!(info.total_ms, 100);
    assert_eq!(info.frames, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_run_reports_fail_and_writes_nothing() {
    let dir = scratch("text-failure");
    let out = dir.join("nope.gif");
    let output = run(&[
        "ascii",
        dir.join("missing.txt").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("FAIL "), "stderr: {stderr}");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_route_fails_with_one_line() {
    let output = run(&["nope"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "stderr: {stderr}");
    assert!(stderr.starts_with("FAIL "));
}
