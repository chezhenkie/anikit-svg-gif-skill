//! Argument parsing, route dispatch and the text route.

use std::path::PathBuf;

use anikit_core::{Dither, Error, MIN_DELAY_MS, Result, WriteOptions, write_gif};
use anikit_render_text::{Face, TextRenderer, layout, load_frames};

use crate::png;

const USAGE: &str = "\
usage: anikit <route> [options]

routes:
  ascii    text frames from a file or a directory of .txt files
  images   a folder of images (not implemented yet)
  vector   an SVG or HTML page (not implemented yet)

ascii options:
  -o, --out PATH      output GIF (default ascii.gif)
  --font PATH         monospace TrueType face to use instead of the bundled one
  --size N            font size in pixels (default 28)
  --width N           fixed canvas width, centres the grid
  --height N          fixed canvas height, requires --width
  --fg COLOUR         default glyph colour (default #e6edf6)
  --bg COLOUR         canvas colour (default #0a0d12)
  --delay MS          default frame delay (default 100)
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0)
  --preview PATH      write the first frame as a PNG";

/// Run one command line.
pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        return Err(Error::invalid(
            "missing route; expected ascii, images or vector",
        ));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        "ascii" => ascii(&args[1..]),
        "images" | "vector" => Err(Error::invalid(format!(
            "route {command} is not implemented yet"
        ))),
        other => Err(Error::invalid(format!(
            "unknown route {other}; expected ascii, images or vector"
        ))),
    }
}

struct Ascii {
    input: PathBuf,
    out: PathBuf,
    font: Option<PathBuf>,
    size: u16,
    width: Option<u32>,
    height: Option<u32>,
    fg: [u8; 3],
    bg: [u8; 3],
    delay: u32,
    colors: usize,
    dither: Dither,
    loop_count: u16,
    preview: Option<PathBuf>,
}

fn ascii(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_ascii(args)?;
    let canvas = match (args.width, args.height) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => {
            return Err(Error::invalid(
                "--width and --height must be given together",
            ));
        }
    };
    if let Some((width, height)) = canvas
        && (width < 8 || height < 8)
    {
        return Err(Error::invalid("canvas width and height must be at least 8"));
    }

    let mut owned: Option<Face> = None;
    let face: &Face = if let Some(path) = &args.font {
        owned = Some(Face::from_path(path)?);
        owned.as_ref().expect("face was just set")
    } else if let Some(path) = env_font() {
        owned = Some(Face::from_path(&path)?);
        owned.as_ref().expect("face was just set")
    } else {
        Face::embedded()
    };
    if owned.is_some() && !face.is_monospace() {
        eprintln!("WARN font is not monospace, columns may not line up");
    }

    let frames = load_frames(&args.input, args.delay.max(MIN_DELAY_MS), args.fg)?;
    let mut renderer = TextRenderer::new(face, args.size)?;
    let grid = layout(&frames, renderer.metrics(), canvas)?;
    let mut images = Vec::with_capacity(frames.len());
    for frame in &frames {
        images.push(renderer.draw(frame, &grid, args.bg)?);
    }

    let delays: Vec<u32> = frames.iter().map(|frame| frame.delay_ms).collect();
    let options = WriteOptions {
        loop_count: args.loop_count,
        colors: args.colors,
        palette_samples: 12,
        dither: args.dither,
    };
    let summary = write_gif(&images, &delays, &args.out, &options)?;

    if let Some(path) = &args.preview {
        png::write_png(path, &images[0])?;
        println!("{}  still frame", path.display());
    }

    let name = args
        .out
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| args.out.display().to_string());
    println!(
        "{}  {} frames, {:.2} s, {} x {}",
        name,
        summary.frames,
        summary.total_ms as f64 / 1000.0,
        summary.width,
        summary.height
    );
    Ok(())
}

fn parse_ascii(args: &[String]) -> Result<Ascii> {
    let mut parsed = Ascii {
        input: PathBuf::new(),
        out: PathBuf::from("ascii.gif"),
        font: None,
        size: 28,
        width: None,
        height: None,
        fg: [0xe6, 0xed, 0xf6],
        bg: [0x0a, 0x0d, 0x12],
        delay: 100,
        colors: 255,
        dither: Dither::None,
        loop_count: 0,
        preview: None,
    };
    let mut input: Option<PathBuf> = None;
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg, None),
        };
        match name {
            "-o" | "--out" => {
                parsed.out = PathBuf::from(take(args, &mut i, inline, name)?);
            }
            "--font" => {
                parsed.font = Some(PathBuf::from(take(args, &mut i, inline, name)?));
            }
            "--size" => {
                parsed.size = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--width" => {
                parsed.width = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--height" => {
                parsed.height = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--fg" => {
                parsed.fg = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--bg" => {
                parsed.bg = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--delay" => {
                parsed.delay = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--colors" => {
                parsed.colors = number::<usize>(&take(args, &mut i, inline, name)?, name)?;
            }
            "--dither" => {
                let value = take(args, &mut i, inline, name)?;
                parsed.dither = match value.as_str() {
                    "none" => Dither::None,
                    "floyd" => Dither::Floyd,
                    _ => {
                        return Err(Error::invalid(format!(
                            "dither must be one of: floyd, none, got {value}"
                        )));
                    }
                };
            }
            "--loop" => {
                parsed.loop_count = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--preview" => {
                parsed.preview = Some(PathBuf::from(take(args, &mut i, inline, name)?));
            }
            other if other.starts_with('-') => {
                return Err(Error::invalid(format!("unknown option {other}")));
            }
            other => {
                if input.is_some() {
                    return Err(Error::invalid(format!("unexpected argument {other}")));
                }
                input = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }
    parsed.input = input.ok_or_else(|| Error::invalid("missing input"))?;
    Ok(parsed)
}

fn take(args: &[String], i: &mut usize, inline: Option<String>, name: &str) -> Result<String> {
    if let Some(value) = inline {
        return Ok(value);
    }
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| Error::invalid(format!("{name} needs a value")))
}

fn number<T: std::str::FromStr>(value: &str, name: &str) -> Result<T> {
    value
        .parse()
        .map_err(|_| Error::invalid(format!("invalid value for {name}: {value}")))
}

fn parse_color(text: &str) -> Result<[u8; 3]> {
    let body = text.trim().trim_start_matches('#');
    if body.contains(',') {
        let parts: Result<Vec<u8>> = body
            .split(',')
            .map(|part| {
                part.parse::<u8>()
                    .map_err(|_| Error::invalid(format!("invalid colour: {text}")))
            })
            .collect();
        let parts = parts?;
        if parts.len() != 3 {
            return Err(Error::invalid(format!("invalid colour: {text}")));
        }
        return Ok([parts[0], parts[1], parts[2]]);
    }
    let hex = if body.len() == 3 {
        body.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        body.to_string()
    };
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::invalid(format!("invalid colour: {text}")));
    }
    let mut rgb = [0u8; 3];
    for (i, byte) in rgb.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| Error::invalid(format!("invalid colour: {text}")))?;
    }
    Ok(rgb)
}

fn env_font() -> Option<PathBuf> {
    let value = std::env::var_os("ANIM_GIF_FONT")?;
    let path = PathBuf::from(value);
    path.is_file().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anikit_core::inspect_file;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/cli")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn delays(gif: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut i = 0;
        while i + 6 < gif.len() {
            if gif[i] == 0x21 && gif[i + 1] == 0xf9 && gif[i + 2] == 0x04 {
                out.push(u16::from_le_bytes([gif[i + 4], gif[i + 5]]) as u32 * 10);
                i += 6;
            } else {
                i += 1;
            }
        }
        out
    }

    #[test]
    fn ascii_route_writes_a_verified_gif() {
        let dir = scratch("ascii-basic");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n--- 250\nbb\n\x1b[32mcc\n").unwrap();
        let out = dir.join("out.gif");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--size",
            "28",
        ]))
        .unwrap();
        let info = inspect_file(&out).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 350);
        assert_eq!((info.width, info.height), (34, 66));
        let bytes = std::fs::read(&out).unwrap();
        assert_eq!(delays(&bytes), vec![100, 250]);
        assert!(!dir.join("out.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixed_canvas_sets_the_size() {
        let dir = scratch("ascii-canvas");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "hi\n").unwrap();
        let out = dir.join("wide.gif");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--width",
            "480",
            "--height",
            "270",
        ]))
        .unwrap();
        let info = inspect_file(&out).unwrap();
        assert_eq!((info.width, info.height), (480, 270));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn runs_are_byte_identical() {
        let dir = scratch("ascii-determinism");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "one\ntwo\n--- 120\nthree\n").unwrap();
        let first = dir.join("first.gif");
        let second = dir.join("second.gif");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            first.to_str().unwrap(),
        ]))
        .unwrap();
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            second.to_str().unwrap(),
        ]))
        .unwrap();
        assert_eq!(
            std::fs::read(&first).unwrap(),
            std::fs::read(&second).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preview_is_a_png_of_the_first_frame() {
        let dir = scratch("ascii-preview");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "ok\n").unwrap();
        let out = dir.join("out.gif");
        let preview = dir.join("still.png");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--preview",
            preview.to_str().unwrap(),
        ]))
        .unwrap();
        let png = std::fs::read(&preview).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        assert_eq!(u32::from_be_bytes([png[16], png[17], png[18], png[19]]), 34);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn identical_frames_merge_into_one_delay() {
        let dir = scratch("ascii-hold");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n---\naa\n").unwrap();
        let out = dir.join("hold.gif");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]))
        .unwrap();
        let info = inspect_file(&out).unwrap();
        assert_eq!(info.frames, 1);
        assert_eq!(info.total_ms, 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_input_fails_without_writing() {
        let dir = scratch("ascii-missing");
        let out = dir.join("out.gif");
        let err = run(&args(&[
            "ascii",
            dir.join("nothere.txt").to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("input not found"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_canvas_needs_both_dimensions() {
        let dir = scratch("ascii-width-only");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "x\n").unwrap();
        let err = run(&args(&["ascii", input.to_str().unwrap(), "--width", "480"])).unwrap_err();
        assert!(err.to_string().contains("together"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn colours_parse_from_hex_and_triplets() {
        assert_eq!(parse_color("#e6edf6").unwrap(), [0xe6, 0xed, 0xf6]);
        assert_eq!(parse_color("#abc").unwrap(), [0xaa, 0xbb, 0xcc]);
        assert_eq!(parse_color("1,2,3").unwrap(), [1, 2, 3]);
        assert!(parse_color("#12345").is_err());
        assert!(parse_color("1,2").is_err());
        assert!(parse_color("nope").is_err());
    }

    #[test]
    fn a_custom_font_replaces_the_bundled_face() {
        let dir = scratch("ascii-font");
        let face = dir.join("missing.ttf");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "x\n").unwrap();
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "--font",
            face.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("cannot read font"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
