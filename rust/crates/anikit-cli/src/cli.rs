//! Argument parsing, route dispatch and the text route.

use std::path::{Component, Path, PathBuf};

use anikit_core::{
    Dither, Error, Frame, FrameSink, MIN_DELAY_MS, OutputFormat, Result, StreamOptions,
    check_canvas, stream_output,
};
use anikit_render_raster::{
    Fit, Mode, Options as SlideshowOptions, extract as extract_animation, read_manifest,
    write_slideshow, write_timed,
};
use anikit_render_svg::{
    ImportOptions, Options as VectorOptions, Preset, TweenOptions, write_animation, write_import,
    write_tween,
};
use anikit_render_text::{Face, TextRenderer, layout, load_frames};
use anikit_time::{FramePlan, LoopMode};

use crate::png;

const USAGE: &str = "\
usage: anikit <route> [options]

routes:
  ascii    text frames from a file or a directory of .txt files
  images   a folder of images
  vector   an SVG or HTML page captured in a browser
  tween    an SVG drawing moved by a JSON timeline
  import   an animated SVG written as a JSON timeline
  extract  frames of a GIF or APNG written to a folder with a manifest

ascii options:
  -o, --out PATH      output .gif or .png/.apng (default ascii.gif)
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
  -o, --out PATH      output .gif or .png/.apng (default slideshow.gif)
  --width N           canvas width (default 800, or the manifest)
  --height N          canvas height (default from the first image, or the manifest)
  --mode MODE         sequence, crossfade or kenburns (default crossfade)
  --fps N             frames per second, 1 to 50 (default 12)
  --hold SECONDS      time each image holds (default 1.2)
  --transition SEC    crossfade length, 0 disables (default 0.5)
  --zoom FACTOR       kenburns end zoom (default 1.10)
  --fit MODE          cover or contain (default cover)
  --bg COLOUR         pad colour for contain (default #000000)
  --timing PATH       per-frame delays from an extract manifest; --fps,
                      --hold, --transition and --zoom are refused
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0, or the manifest)
  --preview PATH      write the first frame as a PNG

vector options:
  -o, --out PATH      output .gif or .png/.apng (default svg.gif)
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
  -o, --out PATH      output .gif, .png/.apng or .svg (default tween.gif)
  --browser PATH      Chromium based browser to drive
  --colors N          palette size, 2 to 255 (default 255)
  --dither MODE       none or floyd (default none)
  --loop N            play count, 0 loops forever (default 0)
  --preview PATH      write the first frame as a PNG

import options:
  -o, --out PATH      output .json timeline (default scene.json)
  --width N           canvas width, default from the drawing
  --height N          canvas height, default from the drawing
  --duration SEC      sampled interval (default 2)
  --fps N             sample rate on the 10 ms grid (default 25)
  --background COLOUR page colour behind the drawing (default #ffffff)
  --loop-mode MODE    once, cycle or pingpong (default cycle)
  --browser PATH      Chromium based browser to drive

extract options:
  -o, --out DIR       output folder for frame-0001.png and manifest.json
                      (default frames)
  --bg COLOUR         matte colour for transparent source pixels (default #000000)";

/// Images route options, parsed from the same helpers as the text route.
struct Images {
    folder: PathBuf,
    out: PathBuf,
    width: u32,
    width_given: bool,
    height: Option<u32>,
    mode: Mode,
    fps: u32,
    fps_given: bool,
    hold: f64,
    hold_given: bool,
    transition: f64,
    transition_given: bool,
    zoom: f64,
    zoom_given: bool,
    fit: Fit,
    bg: [u8; 3],
    timing: Option<PathBuf>,
    colors: usize,
    dither: Dither,
    loop_count: u16,
    loop_given: bool,
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

/// Extract route options: an animation in, a folder of frames out.
struct Extract {
    input: PathBuf,
    out: PathBuf,
    bg: [u8; 3],
}

/// Import route options: an animated SVG in, a JSON timeline out.
struct Import {
    input: PathBuf,
    out: PathBuf,
    width: Option<u32>,
    height: Option<u32>,
    duration: f64,
    fps: u32,
    background: [u8; 3],
    loop_mode: LoopMode,
    browser: Option<PathBuf>,
}

/// Run one command line.
pub fn run(args: &[String]) -> Result<()> {
    let Some(command) = args.first() else {
        return Err(Error::invalid(
            "missing route; expected ascii, images, vector, tween, import or extract",
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
        "import" => import(&args[1..]),
        "extract" => extract(&args[1..]),
        other => Err(Error::invalid(format!(
            "unknown route {other}; expected ascii, images, vector, tween, import or extract"
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
    check_colors(args.colors)?;
    let canvas = match (args.width, args.height) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => {
            return Err(Error::invalid(
                "--width and --height must be given together",
            ));
        }
    };
    if let Some((width, height)) = canvas {
        if width < 8 || height < 8 {
            return Err(Error::invalid("canvas width and height must be at least 8"));
        }
        check_canvas(width, height)?;
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
    check_canvas(grid.width, grid.height)?;
    let format = OutputFormat::from_path(&args.out)?;
    format.require_frames()?;
    check_preview(args.preview.as_deref(), &args.out)?;

    // Palette pass: up to twelve frames spread across the loop, GIF only.
    let mut samples = Vec::new();
    if format == OutputFormat::Gif {
        let picks: Vec<usize> = if frames.len() <= 12 {
            (0..frames.len()).collect()
        } else {
            (0..12).map(|i| i * (frames.len() - 1) / 11).collect()
        };
        for index in &picks {
            samples.push(renderer.draw(&frames[*index], &grid, args.bg)?);
        }
    }

    // Frame starts on the cumulative grid. A frame is pushed when it appears,
    // its delay is the gap to the next push, and the last gap closes the loop.
    let delays: Vec<u32> = frames.iter().map(|frame| frame.delay_ms).collect();
    let plan = FramePlan::cumulative(&delays);

    let options = StreamOptions {
        format,
        width: grid.width,
        height: grid.height,
        loop_count: args.loop_count,
        colors: args.colors,
        dither: args.dither,
        total_ms: plan.total_ms(),
    };
    let mut stream = stream_output(&args.out, &samples, options)?;
    let mut first: Option<Frame> = samples.first().cloned();
    for (index, frame) in frames.iter().enumerate() {
        let image = renderer.draw(frame, &grid, args.bg)?;
        if first.is_none() {
            first = Some(image.clone());
        }
        let pts = plan
            .presentation_us(index)
            .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
        stream.push(index as u32, pts, &image)?;
    }
    let summary = stream.finish()?;

    if let Some(path) = &args.preview {
        let first = first.ok_or_else(|| Error::invalid("no frames to preview"))?;
        png::write_png(path, &first)?;
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
    check_colors(args.colors)?;
    check_preview(args.preview.as_deref(), &args.out)?;
    let rendered = if let Some(path) = &args.timing {
        let mut refused = Vec::new();
        if args.fps_given {
            refused.push("--fps");
        }
        if args.hold_given {
            refused.push("--hold");
        }
        if args.transition_given {
            refused.push("--transition");
        }
        if args.zoom_given {
            refused.push("--zoom");
        }
        if !refused.is_empty() {
            return Err(Error::invalid(format!(
                "{} cannot apply with --timing; the manifest holds the frame delays",
                refused.join(", ")
            )));
        }
        let manifest = read_manifest(path)?;
        let options = SlideshowOptions {
            width: if args.width_given {
                args.width
            } else {
                manifest.width
            },
            height: Some(args.height.unwrap_or(manifest.height)),
            mode: Mode::Sequence,
            fps: args.fps,
            hold: args.hold,
            transition: args.transition,
            zoom: args.zoom,
            fit: args.fit,
            bg: args.bg,
            colors: args.colors,
            dither: args.dither,
            loop_count: if args.loop_given {
                args.loop_count
            } else {
                manifest.loop_count
            },
        };
        write_timed(&args.folder, &args.out, &manifest.delays, &options)?
    } else {
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
        write_slideshow(&args.folder, &args.out, &options)?
    };
    if let Some(path) = &args.preview {
        png::write_png(path, &rendered.first)?;
        println!("{}  still frame", path.display());
    }
    let summary = rendered.summary;
    let name = file_name(&args.out);
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

fn extract(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_extract(args)?;
    let info = extract_animation(&args.input, &args.out, args.bg)?;
    println!(
        "{}  {} frames, {:.2} s, {} x {}",
        file_name(&args.out),
        info.frames,
        info.total_ms as f64 / 1000.0,
        info.width,
        info.height
    );
    Ok(())
}

fn parse_images(args: &[String]) -> Result<Images> {
    let mut parsed = Images {
        folder: PathBuf::new(),
        out: PathBuf::from("slideshow.gif"),
        width: 800,
        width_given: false,
        height: None,
        mode: Mode::Crossfade,
        fps: 12,
        fps_given: false,
        hold: 1.2,
        hold_given: false,
        transition: 0.5,
        transition_given: false,
        zoom: 1.1,
        zoom_given: false,
        fit: Fit::Cover,
        bg: [0, 0, 0],
        timing: None,
        colors: 255,
        dither: Dither::None,
        loop_count: 0,
        loop_given: false,
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
                parsed.width_given = true;
            }
            "--height" => {
                parsed.height = Some(number(&take(args, &mut i, inline, name)?, name)?);
            }
            "--mode" => {
                parsed.mode = parse_mode(&take(args, &mut i, inline, name)?)?;
            }
            "--fps" => {
                parsed.fps = number(&take(args, &mut i, inline, name)?, name)?;
                parsed.fps_given = true;
            }
            "--hold" => {
                parsed.hold = number(&take(args, &mut i, inline, name)?, name)?;
                parsed.hold_given = true;
            }
            "--transition" => {
                parsed.transition = number(&take(args, &mut i, inline, name)?, name)?;
                parsed.transition_given = true;
            }
            "--zoom" => {
                parsed.zoom = number(&take(args, &mut i, inline, name)?, name)?;
                parsed.zoom_given = true;
            }
            "--fit" => {
                parsed.fit = parse_fit(&take(args, &mut i, inline, name)?)?;
            }
            "--bg" => {
                parsed.bg = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--timing" => {
                parsed.timing = Some(PathBuf::from(take(args, &mut i, inline, name)?));
            }
            "--colors" => {
                parsed.colors = number(&take(args, &mut i, inline, name)?, name)?;
            }
            "--dither" => {
                parsed.dither = parse_dither(&take(args, &mut i, inline, name)?)?;
            }
            "--loop" => {
                parsed.loop_count = number(&take(args, &mut i, inline, name)?, name)?;
                parsed.loop_given = true;
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
    check_colors(args.colors)?;
    check_preview(args.preview.as_deref(), &args.out)?;
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
    check_colors(args.colors)?;
    let format = OutputFormat::from_path(&args.out)?;
    if format == OutputFormat::Svg && args.preview.is_some() {
        return Err(Error::invalid("--preview needs a frame-based output"));
    }
    check_preview(args.preview.as_deref(), &args.out)?;
    let options = TweenOptions {
        colors: args.colors,
        dither: args.dither,
        loop_count: args.loop_count,
        browser: args.browser,
    };
    let rendered = write_tween(&args.input, &args.out, &options)?;
    if let Some(path) = &args.preview {
        let first = rendered
            .first
            .as_ref()
            .ok_or_else(|| Error::invalid("no frames to preview"))?;
        png::write_png(path, first)?;
        println!("{}  still frame", path.display());
    }
    let unit = if format == OutputFormat::Svg {
        "samples"
    } else {
        "frames"
    };
    println!(
        "{}  {} {}, {:.2} s, {} x {}",
        file_name(&args.out),
        rendered.summary.frames,
        unit,
        rendered.summary.total_ms as f64 / 1000.0,
        rendered.summary.width,
        rendered.summary.height
    );
    Ok(())
}

fn import(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let args = parse_import(args)?;
    let options = ImportOptions {
        width: args.width,
        height: args.height,
        duration: args.duration,
        fps: args.fps,
        background: args.background,
        loop_mode: args.loop_mode,
        browser: args.browser,
    };
    let imported = write_import(&args.input, &args.out, &options)?;
    for property in &imported.skipped {
        eprintln!("WARN animation of {property} is not imported");
    }
    println!(
        "{}  {} tracks, {:.2} s, {} x {}",
        file_name(&args.out),
        imported.tracks,
        imported.total_ms as f64 / 1000.0,
        imported.width,
        imported.height
    );
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// The palette size is checked for every route that accepts the flag, so a
/// value the GIF path refuses does not pass on APNG or the SVG export.
fn check_colors(colors: usize) -> Result<()> {
    if !(2..=255).contains(&colors) {
        return Err(Error::invalid(format!(
            "--colors must be between 2 and 255, got {colors}"
        )));
    }
    Ok(())
}

/// The preview must not be the output file, which it would replace.
fn check_preview(preview: Option<&Path>, out: &Path) -> Result<()> {
    let Some(preview) = preview else {
        return Ok(());
    };
    if same_path(preview, out) {
        return Err(Error::invalid(format!(
            "--preview {} would replace the output {}",
            preview.display(),
            out.display()
        )));
    }
    Ok(())
}

/// Compare two paths without touching the file system, so a missing output is
/// still compared by name. Windows names are case-insensitive.
fn same_path(a: &Path, b: &Path) -> bool {
    let a = normalized(a);
    let b = normalized(b);
    if cfg!(windows) {
        a.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
    } else {
        a == b
    }
}

/// An absolute path with `.` and `..` resolved lexically.
fn normalized(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
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

fn parse_import(args: &[String]) -> Result<Import> {
    let mut parsed = Import {
        input: PathBuf::new(),
        out: PathBuf::from("scene.json"),
        width: None,
        height: None,
        duration: 2.0,
        fps: 25,
        background: [255, 255, 255],
        loop_mode: LoopMode::Cycle,
        browser: None,
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
            "--background" | "--bg" => {
                parsed.background = parse_color(&take(args, &mut i, inline, name)?)?;
            }
            "--loop-mode" => {
                parsed.loop_mode = parse_loop_mode(&take(args, &mut i, inline, name)?)?;
            }
            "--browser" => {
                parsed.browser = Some(PathBuf::from(take(args, &mut i, inline, name)?));
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

fn parse_extract(args: &[String]) -> Result<Extract> {
    let mut parsed = Extract {
        input: PathBuf::new(),
        out: PathBuf::from("frames"),
        bg: [0, 0, 0],
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
            "--bg" => {
                parsed.bg = parse_color(&take(args, &mut i, inline, name)?)?;
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

fn parse_loop_mode(value: &str) -> Result<LoopMode> {
    match value {
        "once" => Ok(LoopMode::Once),
        "cycle" => Ok(LoopMode::Cycle),
        "pingpong" => Ok(LoopMode::PingPong),
        _ => Err(Error::invalid(format!(
            "loop mode must be one of: cycle, once, pingpong, got {value}"
        ))),
    }
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

    /// Browser tests take this lock so one test binary does not start several
    /// Chromium processes at once on a small runner.
    static BROWSER: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn an_enormous_delay_fails_instead_of_wrapping() {
        let dir = scratch("ascii-delay");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n").unwrap();
        let out = dir.join("out.gif");
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--delay",
            "4294967295",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("delay"), "{err}");
        assert!(!out.exists());

        let separated = dir.join("separated.txt");
        std::fs::write(&separated, "aa\n--- 4294967295\nbb\n").unwrap();
        let err = run(&args(&[
            "ascii",
            separated.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("delay"), "{err}");
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_canvas_above_the_limit_is_refused() {
        let dir = scratch("ascii-limit");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "hi\n").unwrap();
        let out = dir.join("out.gif");
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--width",
            "9000",
            "--height",
            "100",
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("8192"), "{err}");
        assert!(!out.exists());
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
    fn the_preview_cannot_replace_the_output() {
        let dir = scratch("preview-same");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "ok\n").unwrap();
        let out = dir.join("same.gif");
        std::fs::write(&out, b"keep me").unwrap();
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--preview",
            out.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("--preview"), "{err}");
        assert_eq!(std::fs::read(&out).unwrap(), b"keep me");

        let spelled = dir.join(".").join("same.gif");
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--preview",
            spelled.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("--preview"), "{err}");
        assert_eq!(std::fs::read(&out).unwrap(), b"keep me");

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
        assert!(preview.exists());
        assert_eq!(&std::fs::read(&out).unwrap()[..3], b"GIF");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_palette_size_is_checked_on_every_output() {
        let dir = scratch("colors-check");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "ok\n").unwrap();
        for ending in ["gif", "png"] {
            let out = dir.join(format!("out.{ending}"));
            for bad in ["1", "256"] {
                let err = run(&args(&[
                    "ascii",
                    input.to_str().unwrap(),
                    "-o",
                    out.to_str().unwrap(),
                    "--colors",
                    bad,
                ]))
                .unwrap_err();
                assert!(err.to_string().contains("--colors"), "{err}");
                assert!(!out.exists());
            }
        }
        assert!(check_colors(2).is_ok());
        assert!(check_colors(255).is_ok());
        assert!(check_colors(1).is_err());
        assert!(check_colors(256).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_file_has_one_identity() {
        let dir = scratch("same-path");
        let file = dir.join("out.gif");
        assert!(same_path(&file, &file));
        assert!(same_path(&file, &dir.join(".").join("out.gif")));
        assert!(same_path(
            &file,
            &dir.join("sub").join("..").join("out.gif")
        ));
        assert!(!same_path(&file, &dir.join("other.gif")));
        assert!(!same_path(&file, &dir.join("sub").join("out.gif")));
        if cfg!(windows) {
            assert!(same_path(&file, &dir.join("OUT.GIF")));
        }
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
    fn extract_and_rebuild_keep_the_gif_timeline() {
        let dir = scratch("extract-gif");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n--- 250\nbb\n").unwrap();
        let source = dir.join("source.gif");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            source.to_str().unwrap(),
            "--size",
            "12",
        ]))
        .unwrap();
        let before = inspect_file(&source).unwrap();

        let folder = dir.join("frames");
        run(&args(&[
            "extract",
            source.to_str().unwrap(),
            "-o",
            folder.to_str().unwrap(),
        ]))
        .unwrap();
        assert!(folder.join("frame-0001.png").is_file());
        assert!(folder.join("frame-0002.png").is_file());
        let manifest = read_manifest(&folder.join("manifest.json")).unwrap();
        assert_eq!(manifest.delays, vec![100, 250]);

        let rebuilt = dir.join("rebuilt.gif");
        run(&args(&[
            "images",
            folder.to_str().unwrap(),
            "-o",
            rebuilt.to_str().unwrap(),
            "--timing",
            folder.join("manifest.json").to_str().unwrap(),
        ]))
        .unwrap();
        let after = inspect_file(&rebuilt).unwrap();
        assert_eq!(after.frames, before.frames);
        assert_eq!(after.total_ms, before.total_ms);
        assert_eq!((after.width, after.height), (before.width, before.height));
        assert_eq!(delays(&std::fs::read(&rebuilt).unwrap()), vec![100, 250]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_and_rebuild_keep_the_apng_timeline() {
        let dir = scratch("extract-apng");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n--- 250\nbb\n").unwrap();
        let source = dir.join("source.apng");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            source.to_str().unwrap(),
            "--size",
            "12",
        ]))
        .unwrap();
        let before = anikit_core::inspect_apng_file(&source).unwrap();

        let folder = dir.join("frames");
        run(&args(&[
            "extract",
            source.to_str().unwrap(),
            "-o",
            folder.to_str().unwrap(),
        ]))
        .unwrap();
        let rebuilt = dir.join("rebuilt.apng");
        run(&args(&[
            "images",
            folder.to_str().unwrap(),
            "-o",
            rebuilt.to_str().unwrap(),
            "--timing",
            folder.join("manifest.json").to_str().unwrap(),
        ]))
        .unwrap();
        let after = anikit_core::inspect_apng_file(&rebuilt).unwrap();
        assert_eq!(after.frames, before.frames);
        assert_eq!(after.total_ms, before.total_ms);
        assert_eq!((after.width, after.height), (before.width, before.height));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_writes_a_timeline() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        if anikit_render_svg::discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("import-route");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40">
<style>@keyframes slide { from { transform: translateX(0px); } to { transform: translateX(40px); } } #box { animation: slide 1s linear infinite; }</style>
<rect id="box" width="20" height="20" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let out = dir.join("scene.json");
        run(&args(&[
            "import",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--fps",
            "10",
            "--duration",
            "1",
        ]))
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("svg.transform"));
        assert!(text.contains("box-transform"));
        assert!(!dir.join("scene.tmp.json").exists());

        let err = run(&args(&["import", input.to_str().unwrap(), "--fps", "15"])).unwrap_err();
        assert!(err.to_string().contains("multiple of 10 ms"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_timing_manifest_is_refused() {
        let dir = scratch("timing-missing");
        let folder = dir.join("frames");
        std::fs::create_dir_all(&folder).unwrap();
        let mut frame = anikit_core::Frame::new(8, 8).unwrap();
        frame.fill([1, 2, 3]);
        png::write_png(&folder.join("frame-0001.png"), &frame).unwrap();
        let out = dir.join("out.gif");
        let err = run(&args(&[
            "images",
            folder.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--timing",
            dir.join("nothere.json").to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("cannot read"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timing_refuses_flags_it_cannot_use() {
        let dir = scratch("timing-flags");
        let folder = dir.join("frames");
        std::fs::create_dir_all(&folder).unwrap();
        let mut frame = anikit_core::Frame::new(8, 8).unwrap();
        frame.fill([1, 2, 3]);
        png::write_png(&folder.join("frame-0001.png"), &frame).unwrap();
        let manifest = dir.join("manifest.json");
        std::fs::write(
            &manifest,
            "{\"version\":1,\"format\":\"gif\",\"width\":8,\"height\":8,\"loop\":0,\"delays\":[100]}",
        )
        .unwrap();
        let out = dir.join("out.gif");
        for flag in [
            ["--fps", "0"],
            ["--hold", "0.1"],
            ["--transition", "0.1"],
            ["--zoom", "0.1"],
        ] {
            let err = run(&args(&[
                "images",
                folder.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
                "--timing",
                manifest.to_str().unwrap(),
                flag[0],
                flag[1],
            ]))
            .unwrap_err();
            assert!(err.to_string().contains(flag[0]), "{err}");
            assert!(!out.exists(), "{} wrote a file", flag[0]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ascii_route_writes_an_apng() {
        let dir = scratch("ascii-apng");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "aa\n--- 250\nbb\n").unwrap();
        let out = dir.join("out.png");
        run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--size",
            "28",
        ]))
        .unwrap();
        let info = anikit_core::inspect_apng_file(&out).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 350);
        assert_eq!((info.width, info.height), (34, 33));
        assert!(!dir.join("out.tmp.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vector_route_writes_an_apng() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        if anikit_render_svg::discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("vector-apng");
        let input = dir.join("page.html");
        std::fs::write(
            &input,
            "<!doctype html><html><head><style>html,body{margin:0;background:#fff}#box{width:20px;height:20px;background:#c02020}</style></head><body><div id=\"box\"></div></body></html>",
        )
        .unwrap();
        let out = dir.join("out.apng");
        run(&args(&[
            "vector",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--width",
            "40",
            "--height",
            "20",
            "--duration",
            "1",
            "--fps",
            "2",
            "--animate",
            "pulse",
            "--selector",
            "#box",
        ]))
        .unwrap();
        let info = anikit_core::inspect_apng_file(&out).unwrap();
        assert_eq!(info.frames, 2);
        assert_eq!(info.total_ms, 1000);
        assert_eq!((info.width, info.height), (40, 20));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_output_extension_is_refused() {
        let dir = scratch("bad-extension");
        let input = dir.join("frames.txt");
        std::fs::write(&input, "x\n").unwrap();
        let out = dir.join("out.webp");
        let err = run(&args(&[
            "ascii",
            input.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains(".gif, .png, .apng or .svg"));
        assert!(!out.exists());
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
