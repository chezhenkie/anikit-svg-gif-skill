//! Structural comparison against the recorded tween fixture.
//!
//! Part 08 records frame count, delays, size and pixel hashes. This route did
//! not exist before the port, so the reference is the first Rust run: flat art
//! at whole-pixel positions makes the hashes stable across browser builds. The
//! box centre separates a missed sample from a moved palette.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anikit_core::inspect_file;
use sha2::{Digest, Sha256};

#[path = "../../../testsupport/decode.rs"]
mod decode;

const BIN: &str = env!("CARGO_BIN_EXE_anikit");
const WIDTH: usize = 120;

/// Tests that start a browser take this lock, one browser at a time.
static BROWSER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tween")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// One recorded frame: its delay, where the box sat, and its pixels.
#[derive(Debug)]
struct Recorded {
    delay: u32,
    box_x: f64,
    hash: String,
}

fn manifest() -> (HashMap<String, String>, Vec<Recorded>) {
    let text = std::fs::read_to_string(fixture().join("manifest.txt")).expect("manifest");
    let mut keys = HashMap::new();
    let mut frames = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("frame ") {
            let mut delay = 0u32;
            let mut box_x = 0.0;
            let mut hash = String::new();
            for field in rest.split_whitespace().skip(1) {
                let (name, value) = field.split_once('=').expect("frame field");
                match name {
                    "delay_ms" => delay = value.parse().expect("frame delay"),
                    "box_x" => box_x = value.parse().expect("box position"),
                    "hash" => hash = value.to_string(),
                    _ => {}
                }
            }
            frames.push(Recorded { delay, box_x, hash });
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

/// The mean x of pixels that are not the white canvas.
fn box_centre(frame: &[u8]) -> f64 {
    let mut sum = 0.0;
    let mut count = 0.0;
    for (index, pixel) in frame.as_chunks::<4>().0.iter().enumerate() {
        if pixel[..3].iter().any(|channel| *channel < 245) {
            sum += (index % WIDTH) as f64;
            count += 1.0;
        }
    }
    assert!(count > 0.0, "the frame has no box");
    sum / count
}

fn frame_hash(rgba: &[u8]) -> String {
    let mut hasher = Sha256::new();
    for pixel in rgba.as_chunks::<4>().0 {
        hasher.update(&pixel[..3]);
    }
    format!("{:x}", hasher.finalize())
}

#[test]
fn tween_fixture_matches_the_recorded_structure() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping: no Chromium based browser installed");
        return;
    }
    let dir = scratch("tween-fixture");
    let out = dir.join("out.gif");
    let input = fixture().join("input/scene.json");
    let output = run(&[
        "tween",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
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
        let centre = box_centre(buffer);
        assert!(
            (centre - frames[index].box_x).abs() < 2.0,
            "frame {index} box at {centre}, recorded {}",
            frames[index].box_x
        );
    }
    // The box moves right by 6 px per frame, so a timeline that was never
    // sampled cannot pass both ends.
    assert!(frames[9].box_x > frames[0].box_x + 40.0);
    assert_ne!(captured[0].0, captured[9].0);
    assert!(!dir.join("out.tmp.gif").exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stroke_and_stroke_width_are_drawn() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping: no Chromium based browser installed");
        return;
    }
    let dir = scratch("tween-stroke");
    std::fs::write(
        dir.join("line.svg"),
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><rect width="80" height="60" fill="#ffffff"/><rect id="line" x="20" y="20" width="40" height="20" fill="none" stroke="#000000" stroke-width="2"/></svg>"##,
    )
    .unwrap();
    std::fs::write(
        dir.join("scene.json"),
        r##"{
  "version": 1,
  "canvas": {"w": 80, "h": 60, "bg": "#ffffff", "fps": 5},
  "tracks": [
    {"id": "line-colour", "kind": "svg.stroke", "src": "line.svg#line",
     "keys": [{"t": 0, "value": "#000000"}, {"t": 1000, "value": "#ff0000"}]},
    {"id": "line-width", "kind": "svg.stroke-width", "src": "line.svg#line",
     "keys": [{"t": 0, "value": 2}, {"t": 1000, "value": 10}]}
  ]
}
"##,
    )
    .unwrap();
    let out = dir.join("stroke.gif");
    let output = run(&[
        "tween",
        dir.join("scene.json").to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let captured = decoded(&out);
    assert_eq!(captured.len(), 5);
    let red = |frame: &[u8]| {
        frame
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > 180 && pixel[1] < 120 && pixel[2] < 120)
            .count()
    };
    let ink = |frame: &[u8]| {
        frame
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[..3].iter().any(|channel| *channel < 245))
            .count()
    };
    assert_eq!(red(&captured[0].0), 0, "the first stroke is black");
    assert!(red(&captured[4].0) > 100, "the last stroke is red");
    assert!(
        ink(&captured[4].0) > ink(&captured[0].0) * 2,
        "the stroke grew: {} then {}",
        ink(&captured[0].0),
        ink(&captured[4].0)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_runs_are_byte_identical() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping: no Chromium based browser installed");
        return;
    }
    let dir = scratch("tween-determinism");
    let input = fixture().join("input/scene.json");
    let first = dir.join("first.gif");
    let second = dir.join("second.gif");
    for out in [&first, &second] {
        let output = run(&[
            "tween",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]);
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pingpong_doubles_the_plan_and_mirrors_the_second_half() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping: no Chromium based browser installed");
        return;
    }
    let dir = scratch("tween-pingpong");
    let source = std::fs::read_to_string(fixture().join("input/scene.json")).unwrap();
    let ping = source.replace("\"loop\": \"once\"", "\"loop\": \"pingpong\"");
    assert_ne!(ping, source, "the fixture must carry a loop field");
    std::fs::copy(fixture().join("input/art.svg"), dir.join("art.svg")).unwrap();
    let input = dir.join("scene.json");

    let mut outputs = Vec::new();
    for (text, name) in [(&source, "once.gif"), (&ping, "ping.gif")] {
        std::fs::write(&input, text).unwrap();
        let out = dir.join(name);
        let output = run(&[
            "tween",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]);
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        outputs.push(out);
    }
    let once = inspect_file(&outputs[0]).unwrap();
    let mirrored = inspect_file(&outputs[1]).unwrap();
    assert_eq!(once.frames, 10);
    assert_eq!(mirrored.frames, 20);
    assert_eq!(mirrored.total_ms, 2000);
    assert_ne!(
        std::fs::read(&outputs[0]).unwrap(),
        std::fs::read(&outputs[1]).unwrap()
    );

    let frames = decoded(&outputs[1]);
    assert_eq!(frames.len(), 20);
    let end = box_centre(&frames[10].0);
    assert!(
        (end - 80.0).abs() < 2.0,
        "the duration is the end pose: {end}"
    );
    let middle = box_centre(&frames[15].0);
    assert!(
        (middle - 50.0).abs() < 2.0,
        "the second half mirrors: {middle}"
    );
    let back = box_centre(&frames[19].0);
    assert!(
        (back - 26.0).abs() < 2.0,
        "the plan returns to the start: {back}"
    );

    std::fs::write(&input, &ping).unwrap();
    let svg = dir.join("ping.svg");
    let output = run(&[
        "tween",
        input.to_str().unwrap(),
        "-o",
        svg.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&svg).unwrap();
    assert!(
        text.contains("animation:anikit-0 2000ms linear infinite"),
        "the export spans two durations"
    );
    assert!(
        text.contains(
            "50%{transform:matrix(1.000000,0.000000,0.000000,1.000000,60.000000,0.000000)}"
        ),
        "the middle of the export is the end pose"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_svg_export_needs_no_browser_and_plays() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    let dir = scratch("tween-svg");
    let out = dir.join("out.svg");
    let input = fixture().join("input/scene.json");
    let output = run(&[
        "tween",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("samples"), "stdout: {stdout}");

    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("<style id=\"anikit\">"));
    assert!(text.contains("[id=\"box\"]"));
    assert!(text.contains("@keyframes anikit-0{"));
    assert!(text.contains("transform:matrix("));
    assert!(text.contains("animation:anikit-0 1000ms linear forwards"));
    assert!(!text.contains("<script"));
    assert!(!dir.join("out.tmp.svg").exists());

    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping playback: no Chromium based browser installed");
        return;
    }
    let gif = dir.join("played.gif");
    let played = run(&[
        "vector",
        out.to_str().unwrap(),
        "-o",
        gif.to_str().unwrap(),
        "--duration",
        "1",
        "--fps",
        "5",
    ]);
    assert!(
        played.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&played.stderr)
    );
    let captured = decoded(&gif);
    assert_eq!(captured.len(), 5, "the exported CSS must be seekable");
    let first = box_centre(&captured[0].0);
    let last = box_centre(&captured[4].0);
    assert!(
        last > first + 40.0,
        "the exported drawing should move: {first} then {last}"
    );

    // The 100 percent key holds the last sample: 0.95 s and 1.0 s must show
    // the same pose. The first run ends on 0.95 s, the second on 1.0 s.
    let late = dir.join("late.gif");
    let output = run(&[
        "vector",
        out.to_str().unwrap(),
        "-o",
        late.to_str().unwrap(),
        "--duration",
        "1",
        "--fps",
        "20",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let end = dir.join("end.gif");
    let output = run(&[
        "vector",
        out.to_str().unwrap(),
        "-o",
        end.to_str().unwrap(),
        "--duration",
        "1.1",
        "--fps",
        "10",
    ]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let late_frames = decoded(&late);
    let end_frames = decoded(&end);
    let at_095 = box_centre(&late_frames.last().expect("late frame").0);
    let at_100 = box_centre(&end_frames.last().expect("end frame").0);
    assert!(
        (at_095 - at_100).abs() < 2.0,
        "the last sample must hold: {at_095} then {at_100}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_element_reports_its_id_and_writes_nothing() {
    let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
    if anikit_render_svg::discover(None).is_err() {
        eprintln!("skipping: no Chromium based browser installed");
        return;
    }
    let dir = scratch("tween-missing-element");
    let source = std::fs::read_to_string(fixture().join("input/scene.json")).unwrap();
    let scene = source.replace("art.svg#box", "art.svg#not-there");
    let input = dir.join("scene.json");
    std::fs::write(&input, scene).unwrap();
    std::fs::copy(fixture().join("input/art.svg"), dir.join("art.svg")).unwrap();
    let out = dir.join("nope.gif");
    let output = run(&[
        "tween",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("FAIL "), "stderr: {stderr}");
    assert!(stderr.contains("element not found"), "stderr: {stderr}");
    assert!(stderr.contains("not-there"), "stderr: {stderr}");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_scene_reports_the_place_and_writes_nothing() {
    let dir = scratch("tween-bad-scene");
    let input = dir.join("scene.json");
    std::fs::write(&input, "{\"version\": 1, \"canvas\": {}}\n").unwrap();
    let out = dir.join("nope.gif");
    let output = run(&[
        "tween",
        input.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("FAIL "), "stderr: {stderr}");
    assert!(stderr.contains("scene.json.canvas"), "stderr: {stderr}");
    assert!(stderr.contains("missing field \"w\""), "stderr: {stderr}");
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}
