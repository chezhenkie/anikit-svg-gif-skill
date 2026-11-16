//! Argument parsing, route dispatch and the text route.

use std::path::{Path, PathBuf};

use anikit_core::{
    Dither, Error, FrameSink, MIN_DELAY_MS, Result, StreamOptions, snap_delay, stream_gif,
};
use anikit_render_raster::{Fit, Mode, Options as SlideshowOptions, write_slideshow};
use anikit_render_svg::{
    Options as VectorOptions, Preset, TweenOptions, write_animation, write_tween,
};
use anikit_render_text::{Face, TextRenderer, layout, load_frames};

use crate::png;

const USAGE: &str = "\
usage: anikit <route> [options]

routes:
  ascii    text frames from a file or a directory of .txt files
  images   a folder of images
  vector   an SVG or HTML page captured in a browser
  tween    an SVG drawing moved by a JSON timeline

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
  --preview PATH      write the first frame as a PNG

images options:
  -o, --out PATH      output GIF (default slideshow.gif)
  --width N           canvas width (default 800)
  --height N          canvas height (default from the first image)
  --mode MODE         sequence, crossfade or kenburns (default crossfade)
  --fps N             frames per second, 1 to 50 (default 12)
  --hold SECONDS      time each image holds (default 1.2)
  --transition SEC    crossfade length, 0 disables (default 0.5)
  --zoom FACTOR       kenburns end zoom (default 1.10)
  --fit MODE          cover or contain (default cover)
  --bg COLOUR         pad colour for contain (default #000000)
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0)
  --preview PATH      write the first frame as a PNG

vector options:
  -o, --out PATH      output GIF (default svg.gif)
  --width N           viewport width, default from the drawing
  --height N          viewport height, default from the drawing
  --duration SEC      seconds per animation cycle (default 2)
  --fps N             frames per second, 1 to 50 (default 15)
  --animate PRESET    none, rotate, pulse or fade (default none)
  --selector CSS      preset target, default the svg element
  --background COLOUR page colour behind the drawing (default #ffffff)
  --dpr N             device pixel ratio, 1 or 2 (default 1)
  --browser PATH      Chromium based browser to drive
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0)
  --preview PATH      write the first frame as a PNG

tween options:
  -o, --out PATH      output GIF (default tween.gif)
  --browser PATH      Chromium based browser to drive
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0)
  --preview PATH      write the first frame as a PNG";

/// Images route options, parsed from the same helpers as the text route.
struct Images {
    folder: PathBuf,
    out: PathBuf,
    width: u32,
    height: Option<u32>,
    mode: Mode,
    fps: u32,
    hold: f64,
    transition: f64,
    zoom: f64,
    fit: Fit,
    bg: [u8; 3],
    colors: usize,
    dither: Dither,
    loop_count: u16,
    preview: Option<PathBuf>,
}

/// Vector route options.
struct Vector {
    input: PathBuf,
    out: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    duration: f64,
    fps: u32,
    animate: Preset,
    selector: Option<String>,
    background: [u8; 3],
    dpr: u32,
    browser: Option<PathBuf>,
    colors: usize,
    dither: Dither,
    loop_count: u16,
    preview: Option<PathBuf>,
}

/// Tween route options. The timeline owns size, fps and timing.
struct Tween {
    input: PathBuf,
    out: PathBuf,
    browser: Option<PathBuf>,
    colors: usize,
    dither: Dither,
    loop_count: u16,
    preview: Option<PathBuf>,
}

/// Run one command line.
pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        return Err(Error::invalid(
            "missing route; expected ascii, images, vector or tween",
        ));
    };
    match command.as_str() {
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        "ascii" => ascii(&args[1..]),
        "images" => images(&args[1..]),
        "vector" => vector(&args[1..]),
        "tween" => tween(&args[1..]),
        other => Err(Error::invalid(format!(
            "unknown route {other}; expected ascii, images, vector or tween"
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

    // Palette pass: up to twelve frames spread across the loop.
    let picks: Vec<usize> = if frames.len() <= 12 {
        (0..frames.len()).collect()
    } else {
        (0..12).map(|i| i * (frames.len() - 1) / 11).collect()
    };
    let mut samples = Vec::with_capacity(picks.len());
    for index in &picks {
        samples.push(renderer.draw(&frames[*index], &grid, args.bg)?);
    }

    // Presentation times from the per-frame delays, on the cumulative grid.
    // A frame is pushed at the moment it appears, so its delay is the gap to
    // the next push and the loop length closes the last one.
    let mut times: Vec<u64> = Vec::with_capacity(frames.len());
    let mut total_ms = 0u32;
    for frame in &frames {
        times.push(total_ms as u64 * 1000);
        total_ms += snap_delay(frame.delay_ms);
    }

    let options = StreamOptions {
        loop_count: args.loop_count,
        colors: args.colors,
        dither: args.dither,
        total_ms,
    };
    let mut stream = stream_gif(&args.out, &samples, options)?;
    for (index, frame) in frames.iter().enumerate() {
        let image = renderer.draw(frame, &grid, args.bg)?;
        stream.push(index as u32, times[index], &image)?;
    }
    let summary = stream.finish()?;

    if let Some(path) = &args.preview {
        png::write_png(path, &samples[0])?;
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

fn images(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_images(args)?;
    let options = SlideshowOptions {
        width: args.width,
        height: args.height,
        mode: args.mode,
        fps: args.fps,
        hold: args.hold,
        transition: args.transition,
        zoom: args.zoom,
        fit: args.fit,
        bg: args.bg,
        colors: args.colors,
        dither: args.dither,
        loop_count: args.loop_count,
    };
    let rendered = write_slideshow(&args.folder, &args.out, &options)?;
    if let Some(path) = &args.preview {
        png::write_png(path, &rendered.first)?;
        println!("{}  still frame", path.display());
    }
    let summary = rendered.summary;
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

fn parse_images(args: &[String]) -> Result<Images> {
    let mut parsed = Images {
        folder: PathBuf::new(),
        out: PathBuf::from("slideshow.gif"),
        width: 800,
        height: None,
        mode: Mode::Crossfade,
        fps: 12,
        hold: 1.2,
        transition: 0.5,
        zoom: 1.1,
        fit: Fit::Cover,
        bg: [0, 0, 0],
        colors: 255,
        dither: Dither::None,
        loop_count: 0,
        preview: None,
    };
    let mut folder: Option<PathBuf> = None;
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
            "--width" => {
                parsed.width = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--height" => {
                parsed.height = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--mode" => {
                parsed.mode = parse_mode(&take(args, &mut i, inline, name)?)?;
            }
            "--fps" => {
                parsed.fps = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--hold" => {
                parsed.hold = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--transition" => {
                parsed.transition = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--zoom" => {
                parsed.zoom = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--fit" => {
                parsed.fit = parse_fit(&take(args, &mut i, inline, name)?)?;
            }
            "--bg" => {
                parsed.bg = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--colors" => {
                parsed.colors = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--dither" => {
                parsed.dither = parse_dither(&take(args, &mut i, inline, name)?)?;
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
                if folder.is_some() {
                    return Err(Error::invalid(format!("unexpected argument {other}")));
                }
                folder = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }
    parsed.folder = folder.ok_or_else(|| Error::invalid("missing folder"))?;
    Ok(parsed)
}

fn vector(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_vector(args)?;
    let options = VectorOptions {
        width: args.width,
        height: args.height,
        duration: args.duration,
        fps: args.fps,
        animate: args.animate,
        selector: args.selector,
        background: args.background,
        dpr: args.dpr,
        browser: args.browser,
        colors: args.colors,
        dither: args.dither,
        loop_count: args.loop_count,
    };
    let rendered = write_animation(&args.input, &args.out, &options)?;
    if let Some(path) = &args.preview {
        png::write_png(path, &rendered.first)?;
        println!("{}  still frame", path.display());
    }
    println!(
        "{}  {} frames, {:.2} s, {} x {}",
        file_name(&args.out),
        rendered.summary.frames,
        rendered.summary.total_ms as f64 / 1000.0,
        rendered.summary.width,
        rendered.summary.height
    );
    Ok(())
}

fn tween(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_tween(args)?;
    let options = TweenOptions {
        colors: args.colors,
        dither: args.dither,
        loop_count: args.loop_count,
        browser: args.browser,
    };
    let rendered = write_tween(&args.input, &args.out, &options)?;
    if let Some(path) = &args.preview {
        png::write_png(path, &rendered.first)?;
        println!("{}  still frame", path.display());
    }
    println!(
        "{}  {} frames, {:.2} s, {} x {}",
        file_name(&args.out),
        rendered.summary.frames,
        rendered.summary.total_ms as f64 / 1000.0,
        rendered.summary.width,
        rendered.summary.height
    );
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn parse_vector(args: &[String]) -> Result<Vector> {
    let mut parsed = Vector {
        input: PathBuf::new(),
        out: PathBuf::from("svg.gif"),
        width: None,
        height: None,
        duration: 2.0,
        fps: 15,
        animate: Preset::None,
        selector: None,
        background: [255, 255, 255],
        dpr: 1,
        browser: None,
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
            "--width" => {
                parsed.width = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--height" => {
                parsed.height = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--duration" => {
                parsed.duration = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--fps" => {
                parsed.fps = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--animate" => {
                parsed.animate = Preset::parse(&take(args, &mut i, inline, name)?)?;
            }
            "--selector" => {
                parsed.selector = Some(take(args, &mut i, inline, name)?);
            }
            "--background" | "--bg" => {
                parsed.background = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--dpr" => {
                parsed.dpr = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--browser" => {
                parsed.browser = Some(PathBuf::from(take(args, &mut i, inline, name)?));
            }
            "--colors" => {
                parsed.colors = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--dither" => {
                parsed.dither = parse_dither(&take(args, &mut i, inline, name)?)?;
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

fn parse_tween(args: &[String]) -> Result<Tween> {
    let mut parsed = Tween {
        input: PathBuf::new(),
        out: PathBuf::from("tween.gif"),
        browser: None,
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
            "--browser" => {
                parsed.browser = Some(PathBuf::from(take(args, &mut i, inline, name)?));
            }
            "--colors" => {
                parsed.colors = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--dither" => {
                parsed.dither = parse_dither(&take(args, &mut i, inline, name)?)?;
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

fn parse_mode(value: &str) -> Result<Mode> {
    match value {
        "sequence" => Ok(Mode::Sequence),
        "crossfade" => Ok(Mode::Crossfade),
        "kenburns" => Ok(Mode::KenBurns),
        _ => Err(Error::invalid(format!(
            "mode must be one of: crossfade, kenburns, sequence, got {value}"
        ))),
    }
}

fn parse_fit(value: &str) -> Result<Fit> {
    match value {
        "cover" => Ok(Fit::Cover),
        "contain" => Ok(Fit::Contain),
        _ => Err(Error::invalid(format!(
            "fit must be one of: contain, cover, got {value}"
        ))),
    }
}

fn parse_dither(value: &str) -> Result<Dither> {
    match value {
        "none" => Ok(Dither::None),
        "floyd" => Ok(Dither::Floyd),
        _ => Err(Error::invalid(format!(
            "dither must be one of: floyd, none, got {value}"
        ))),
    }
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
                parsed.dither = parse_dither(&take(args, &mut i, inline, name)?)?;
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
    fn images_route_writes_a_sequence_of_stills() {
        let dir = scratch("images-sequence");
        let folder = dir.join("frames");
        std::fs::create_dir_all(&folder).unwrap();
        let mut red = anikit_core::Frame::new(8, 8).unwrap();
        red.fill([255, 0, 0]);
        let mut blue = anikit_core::Frame::new(8, 8).unwrap();
        blue.fill([0, 0, 255]);
        png::write_png(&folder.join("1.png"), &red).unwrap();
        png::write_png(&folder.join("2.png"), &blue).unwrap();

        let out = dir.join("reel.gif");
        run(&args(&[
            "images",
            folder.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--mode",
            "sequence",
            "--fps",
            "10",
            "--hold",
            "0.3",
            "--width",
            "40",
            "--height",
            "20",
        ]))
        .unwrap();
        let info = inspect_file(&out).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 600);
        assert_eq!((info.width, info.height), (40, 20));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_route_writes_a_preview() {
        let dir = scratch("images-preview");
        let folder = dir.join("frames");
        std::fs::create_dir_all(&folder).unwrap();
        let mut frame = anikit_core::Frame::new(8, 8).unwrap();
        frame.fill([10, 20, 30]);
        png::write_png(&folder.join("only.png"), &frame).unwrap();
        let out = dir.join("reel.gif");
        let preview = dir.join("still.png");
        run(&args(&[
            "images",
            folder.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--width",
            "40",
            "--height",
            "20",
            "--preview",
            preview.to_str().unwrap(),
        ]))
        .unwrap();
        let png = std::fs::read(&preview).unwrap();
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn images_route_needs_a_folder() {
        let dir = scratch("images-missing");
        let err = run(&args(&["images", dir.join("nothere").to_str().unwrap()])).unwrap_err();
        assert!(err.to_string().contains("folder not found"));
        let _ = std::fs::remove_dir_all(&dir);
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
