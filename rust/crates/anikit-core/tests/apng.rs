//! APNG round-trip against the png crate's decoder (part 12).

use std::path::PathBuf;

use anikit_core::{
    Dither, Frame, FrameSink, OutputFormat, StreamOptions, inspect_apng_file, stream_output,
};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn solid(rgb: [u8; 3]) -> Frame {
    let mut frame = Frame::new(4, 3).unwrap();
    frame.fill(rgb);
    frame
}

fn write(path: &std::path::Path) {
    let red = solid([255, 0, 0]);
    let green = solid([0, 255, 0]);
    let blue = solid([0, 0, 255]);
    let options = StreamOptions {
        format: OutputFormat::Apng,
        width: 4,
        height: 3,
        loop_count: 3,
        colors: 255,
        dither: Dither::None,
        total_ms: 300,
    };
    let mut stream = stream_output(path, &[], options).unwrap();
    stream.push(0, 0, &red).unwrap();
    stream.push(1, 100_000, &red).unwrap();
    stream.push(2, 200_000, &blue).unwrap();
    stream.push(3, 250_000, &blue).unwrap();
    stream.push(4, 260_000, &green).unwrap();
    let summary = stream.finish().unwrap();
    assert_eq!(summary.frames, 3);
    assert_eq!(summary.total_ms, 300);
}

#[test]
fn a_written_apng_decodes_in_the_png_crate() {
    let dir = scratch("apng-roundtrip");
    let path = dir.join("out.png");
    write(&path);

    let info = inspect_apng_file(&path).unwrap();
    assert_eq!(info.frames, 3);
    assert_eq!(info.total_ms, 300);
    assert_eq!((info.width, info.height), (4, 3));

    let file = std::io::BufReader::new(std::fs::File::open(&path).unwrap());
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().unwrap();
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let mut frames = 0;
    while let Ok(frame) = reader.next_frame(&mut buffer) {
        assert_eq!((frame.width, frame.height), (4, 3));
        frames += 1;
        if frames == 3 {
            break;
        }
    }
    assert_eq!(frames, 3);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn truth_and_loop_count_come_from_the_control_chunk() {
    let dir = scratch("apng-control");
    let path = dir.join("out.apng");
    write(&path);
    let bytes = std::fs::read(&path).unwrap();
    let control = bytes
        .windows(4)
        .position(|window| window == b"acTL")
        .expect("animation control");
    let frames = u32::from_be_bytes([
        bytes[control + 4],
        bytes[control + 5],
        bytes[control + 6],
        bytes[control + 7],
    ]);
    let plays = u32::from_be_bytes([
        bytes[control + 8],
        bytes[control + 9],
        bytes[control + 10],
        bytes[control + 11],
    ]);
    assert_eq!(frames, 3);
    assert_eq!(plays, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_longest_legal_delay_is_written_and_read() {
    let dir = scratch("apng-max-delay");
    let path = dir.join("long.apng");
    let red = solid([255, 0, 0]);
    let options = StreamOptions {
        format: OutputFormat::Apng,
        width: 4,
        height: 3,
        loop_count: 0,
        colors: 255,
        dither: Dither::None,
        total_ms: 655_350,
    };
    let mut stream = stream_output(&path, &[], options).unwrap();
    stream.push(0, 0, &red).unwrap();
    let summary = stream.finish().unwrap();
    assert_eq!(summary.total_ms, 655_350);
    let info = inspect_apng_file(&path).unwrap();
    assert_eq!(info.frames, 1);
    assert_eq!(info.total_ms, 655_350);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_delay_past_the_maximum_is_refused() {
    let dir = scratch("apng-too-long");
    let path = dir.join("long.apng");
    let red = solid([255, 0, 0]);
    let options = StreamOptions {
        format: OutputFormat::Apng,
        width: 4,
        height: 3,
        loop_count: 0,
        colors: 255,
        dither: Dither::None,
        total_ms: 655_360,
    };
    let mut stream = stream_output(&path, &[], options).unwrap();
    stream.push(0, 0, &red).unwrap();
    let err = stream.finish().unwrap_err();
    assert!(err.to_string().contains("655360"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_corrupt_chunk_is_caught_before_the_file_is_accepted() {
    let dir = scratch("apng-corrupt");
    let path = dir.join("out.png");
    write(&path);
    let mut bytes = std::fs::read(&path).unwrap();
    // Flip one byte inside the first IDAT payload; the checksum must notice.
    let at = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .expect("image data");
    bytes[at + 8] ^= 0x01;
    assert!(anikit_core::inspect_apng(&bytes).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
