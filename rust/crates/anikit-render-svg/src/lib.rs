//! The vector route: browser discovery, page construction, seek and capture.
//!
//! One browser is started per run, the page is loaded once, and every frame is
//! a paused clock set to a presentation time and captured as a PNG. Frames are
//! pushed straight into the core stream, so the loop is never held as images.

mod browser;
pub(crate) mod cdp;
mod export;
mod import;
pub(crate) mod page;
mod timeline;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anikit_core::{
    Dither, Error, Frame, FrameSink, OutputFormat, OutputSummary, Result, StreamOptions,
    check_canvas, stream_output,
};
use anikit_time::FramePlan;
use serde_json::{Value, json};

use browser::Browser;
use cdp::Session;

pub use browser::discover;
pub use import::{ImportOptions, Imported, write_import};
pub use page::{Preset, SvgSize, resolve_size, svg_size};
pub use timeline::{TweenOptions, TweenRendered, write_tween};

/// How many frames feed the palette when the loop is longer than that.
const SAMPLES: usize = 12;

/// Where the input came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Svg,
    Html,
}

/// Everything the vector route needs that is not an input or an output path.
#[derive(Clone, Debug)]
pub struct Options {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Seconds per animation cycle.
    pub duration: f64,
    pub fps: u32,
    pub animate: Preset,
    pub selector: Option<String>,
    pub background: [u8; 3],
    pub dpr: u32,
    pub browser: Option<PathBuf>,
    pub colors: usize,
    pub dither: Dither,
    pub loop_count: u16,
}

/// What the route wrote, plus the first frame for an optional preview.
#[derive(Debug)]
pub struct Rendered {
    pub summary: OutputSummary,
    pub first: Frame,
}

/// Capture one SVG or HTML animation into a GIF or an APNG.
pub fn write_animation(input: &Path, out: &Path, options: &Options) -> Result<Rendered> {
    let format = OutputFormat::from_path(out)?;
    format.require_frames()?;
    let kind = Kind::of(input)?;
    validate(options)?;
    let svg = match kind {
        Kind::Svg => Some(
            std::fs::read_to_string(input)
                .map_err(|err| Error::io(format!("cannot read {}: {err}", input.display())))?,
        ),
        Kind::Html => None,
    };
    let (width, height) = resolve_size(svg.as_deref(), options.width, options.height)?;
    if width < 8 || height < 8 {
        return Err(Error::invalid("width and height must be at least 8"));
    }
    check_canvas(width, height)?;
    // The page stays at the logical size; the screenshot and the output carry
    // one pixel per device pixel, so dpr 2 doubles both sides.
    let canvas_width = width.saturating_mul(options.dpr);
    let canvas_height = height.saturating_mul(options.dpr);

    let fps = options.fps as f64;
    let count = (page::round_half_even(options.duration * fps) as i64).max(2) as usize;
    let plan = FramePlan::uniform(count, fps)?;

    let exe = discover(options.browser.as_deref())?;

    // An SVG is drawn in a page of our own; the base URL keeps its relative
    // references working. The guard removes the page before the run returns.
    let temp = match &svg {
        Some(text) => Some(write_page(input, text, width, height, options.background)?),
        None => None,
    };
    let target = match &temp {
        Some(page) => page::file_url(&page.path)?,
        None => page::file_url(input)?,
    };
    let mut page = Page::open(
        &exe,
        width,
        height,
        options.dpr,
        &target,
        options.background,
    )?;
    let setup = page::js_setup(
        kind == Kind::Html,
        options.animate,
        options.selector.as_deref(),
        options.duration,
        options.background,
    );
    page.evaluate(&setup, true)?;

    let picks = SAMPLES.min(count);
    let mut samples = Vec::with_capacity(picks);
    if format == OutputFormat::Gif {
        for pick in 0..picks {
            let index = if count <= picks {
                pick
            } else {
                (pick as f64 * (count - 1) as f64 / (picks - 1) as f64).round() as usize
            };
            samples.push(page.frame(presentation_seconds(&plan, index)?)?);
        }
    }

    let mut stream = stream_output(
        out,
        &samples,
        StreamOptions {
            format,
            width: canvas_width,
            height: canvas_height,
            loop_count: options.loop_count,
            colors: options.colors,
            dither: options.dither,
            total_ms: plan.total_ms(),
        },
    )?;

    let mut first: Option<Frame> = None;
    for index in 0..count {
        let pts = plan
            .presentation_us(index)
            .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
        let frame = page.frame(pts as f64 / 1_000_000.0)?;
        if first.is_none() {
            first = Some(frame.clone());
        }
        stream.push(index as u32, pts, &frame)?;
    }
    drop(page);
    let summary = stream.finish()?;
    let first = first.ok_or_else(|| Error::invalid("no frames were captured"))?;
    Ok(Rendered { summary, first })
}

impl Kind {
    fn of(input: &Path) -> Result<Kind> {
        if !input.is_file() {
            return Err(Error::invalid(format!(
                "input not found: {}",
                input.display()
            )));
        }
        match input
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("svg") => Ok(Kind::Svg),
            Some("html") | Some("htm") => Ok(Kind::Html),
            _ => Err(Error::invalid("input must be .svg, .html or .htm")),
        }
    }
}

fn validate(options: &Options) -> Result<()> {
    if !options.duration.is_finite() || options.duration <= 0.0 {
        return Err(Error::invalid("duration must be greater than 0"));
    }
    if !(1..=anikit_time::MAX_FPS as u32).contains(&options.fps) {
        return Err(Error::invalid(format!(
            "fps must be between 1 and {}",
            anikit_time::MAX_FPS as u32
        )));
    }
    if options.dpr != 1 && options.dpr != 2 {
        return Err(Error::invalid("dpr must be 1 or 2"));
    }
    Ok(())
}

/// The time one frame is seeked to, in seconds, taken from the plan.
fn presentation_seconds(plan: &FramePlan, index: usize) -> Result<f64> {
    let us = plan
        .presentation_us(index)
        .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
    Ok(us as f64 / 1_000_000.0)
}

/// A page file that removes itself.
struct PageDir {
    dir: PathBuf,
    path: PathBuf,
}

impl Drop for PageDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Write the wrapping page beside no input, under the system temp directory.
pub(crate) fn write_page(
    input: &Path,
    svg: &str,
    width: u32,
    height: u32,
    background: [u8; 3],
) -> Result<PageDir> {
    let parent = input
        .canonicalize()
        .map_err(|err| Error::io(format!("cannot read {}: {err}", input.display())))?
        .parent()
        .map(|dir| dir.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let base = format!("{}/", page::file_url(&parent)?);
    let dir = browser::unique_dir("page")?;
    let path = dir.join("page.html");
    std::fs::write(&path, page::wrap_svg(svg, &base, width, height, background))
        .map_err(|err| Error::io(format!("cannot write {}: {err}", path.display())))?;
    Ok(PageDir { dir, path })
}

/// Poll the document until it has loaded and its URL is no longer about:blank.
fn wait_for_load(session: &mut Session) -> Result<()> {
    let probe = "document.readyState + '|' + document.location.href";
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        // A killed evaluation context during navigation is expected; the probe
        // is repeated until the new document answers.
        if let Ok(reply) = evaluate(session, probe, false)
            && let Some(text) = reply.pointer("/result/value").and_then(Value::as_str)
            && let Some((state, href)) = text.split_once('|')
            && state == "complete"
            && !href.starts_with("about:")
        {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(Error::invalid(
                "the page did not finish loading within 30 s",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run a script in the page and surface its exception as a route error.
fn evaluate(session: &mut Session, expression: &str, await_promise: bool) -> Result<Value> {
    let reply = session.call(
        "Runtime.evaluate",
        Some(json!({
            "expression": expression,
            "awaitPromise": await_promise,
            "returnByValue": true,
        })),
    )?;
    if let Some(details) = reply.get("exceptionDetails") {
        let text = details
            .get("exception")
            .and_then(|exception| exception.get("description"))
            .and_then(Value::as_str)
            .or_else(|| details.get("text").and_then(Value::as_str))
            .unwrap_or("script error");
        let line = text.lines().next().unwrap_or("script error");
        return Err(Error::invalid(format!("page script failed: {line}")));
    }
    Ok(reply)
}

/// A launched browser with one page target, sized and navigated.
///
/// The browser is the field that is never read: it must outlive the session and
/// kills its process and profile when the page is dropped.
pub(crate) struct Page {
    capturer: Capturer,
    _browser: Browser,
}

impl Page {
    pub(crate) fn open(
        exe: &Path,
        width: u32,
        height: u32,
        dpr: u32,
        target: &str,
        background: [u8; 3],
    ) -> Result<Page> {
        let mut browser = Browser::launch(exe)?;
        let socket = browser.page_web_socket()?;
        let mut capturer = Capturer {
            session: Session::connect(&socket)?,
            background,
        };
        capturer.session.call(
            "Emulation.setDeviceMetricsOverride",
            Some(json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": dpr,
                "mobile": false,
            })),
        )?;
        capturer
            .session
            .call("Page.navigate", Some(json!({ "url": target })))?;
        wait_for_load(&mut capturer.session)?;
        Ok(Page {
            capturer,
            _browser: browser,
        })
    }

    /// Run a script in the page.
    pub(crate) fn evaluate(&mut self, script: &str, await_promise: bool) -> Result<Value> {
        evaluate(&mut self.capturer.session, script, await_promise)
    }

    /// Take one screenshot, decoded and matted onto the background.
    pub(crate) fn shoot(&mut self) -> Result<Frame> {
        self.capturer.shoot()
    }

    /// Seek every clock to one time and take a screenshot.
    fn frame(&mut self, seconds: f64) -> Result<Frame> {
        self.evaluate(&page::js_seek(seconds), false)?;
        self.shoot()
    }
}

/// A page that can be captured.
struct Capturer {
    session: Session,
    background: [u8; 3],
}

impl Capturer {
    fn shoot(&mut self) -> Result<Frame> {
        let reply = self.session.call(
            "Page.captureScreenshot",
            Some(json!({
                "format": "png",
                "fromSurface": true,
                "captureBeyondViewport": false,
            })),
        )?;
        let data = reply
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("the browser sent an empty screenshot"))?;
        decode_png(&base64_decode(data)?, self.background)
    }
}

/// The base64 alphabet, decoded strictly.
fn base64_decode(text: &str) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return Err(Error::invalid("the screenshot is not base64")),
        };
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

/// A screenshot as an opaque frame, alpha multiplied into the background.
fn decode_png(png: &[u8], background: [u8; 3]) -> Result<Frame> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|err| Error::invalid(format!("cannot decode the screenshot: {err}")))?
        .to_rgba8();
    let (width, height) = image.dimensions();
    let mut frame = Frame::new(width, height)?;
    let data = frame.data_mut();
    for (index, pixel) in image.pixels().enumerate() {
        let alpha = pixel[3] as u32;
        let keep = 255 - alpha;
        let at = index * 3;
        for channel in 0..3 {
            data[at + channel] =
                ((pixel[channel] as u32 * alpha + background[channel] as u32 * keep + 127) / 255)
                    as u8;
        }
    }
    Ok(frame)
}

#[cfg(test)]
pub(crate) mod test_lock {
    /// Tests that launch a browser take this lock, so one binary does not start
    /// several Chromium processes at once on a small runner.
    pub static BROWSER: std::sync::Mutex<()> = std::sync::Mutex::new(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_lock::BROWSER;

    fn options() -> Options {
        Options {
            width: None,
            height: None,
            duration: 1.0,
            fps: 4,
            animate: Preset::None,
            selector: None,
            background: [255, 255, 255],
            dpr: 1,
            browser: None,
            colors: 255,
            dither: Dither::None,
            loop_count: 0,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/vector")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A CSS move and a SMIL move, both linear over one second.
    const DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
<style>@keyframes slide { from { transform: translateX(0px); } to { transform: translateX(40px); } } .mover { animation: slide 1s linear; }</style>
<rect width="80" height="40" fill="#ffffff"/>
<rect class="mover" x="0" y="0" width="20" height="20" fill="#ff0000"/>
<rect x="0" y="20" width="20" height="20" fill="#0000ff"><animate attributeName="x" from="0" to="60" dur="1s" fill="freeze"/></rect>
</svg>"##;

    fn decoded(path: &Path) -> Vec<Vec<u8>> {
        let bytes = std::fs::read(path).unwrap();
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            frames.push(frame.buffer.to_vec());
        }
        frames
    }

    /// The mean x of pixels near one colour, or None when the frame has none.
    fn centre(frame: &[u8], width: usize, want: [u8; 3]) -> Option<f64> {
        let mut sum = 0.0;
        let mut count = 0.0;
        for (index, pixel) in frame.as_chunks::<4>().0.iter().enumerate() {
            let close =
                (0..3).all(|channel| (pixel[channel] as i32 - want[channel] as i32).abs() < 48);
            if close {
                sum += (index % width) as f64;
                count += 1.0;
            }
        }
        (count > 0.0).then_some(sum / count)
    }

    #[test]
    fn base64_round_trips_through_a_screenshot() {
        let text = "aGVsbG8=";
        assert_eq!(base64_decode(text).unwrap(), b"hello");
        assert_eq!(base64_decode("").unwrap(), Vec::<u8>::new());
        assert!(base64_decode("a$b").is_err());
    }

    #[test]
    fn an_opaque_screenshot_keeps_its_colours() {
        let mut png = Vec::new();
        {
            let image = image::RgbaImage::from_pixel(2, 1, image::Rgba([10, 20, 30, 255]));
            let encoder = image::codecs::png::PngEncoder::new(&mut png);
            use image::ImageEncoder;
            encoder
                .write_image(image.as_raw(), 2, 1, image::ExtendedColorType::Rgba8)
                .unwrap();
        }
        let frame = decode_png(&png, [0, 0, 0]).unwrap();
        assert_eq!(frame.size(), (2, 1));
        assert_eq!(frame.pixel(1, 0), [10, 20, 30]);
    }

    #[test]
    fn bad_options_are_rejected_before_a_browser_starts() {
        let dir = scratch("bad-options");
        let input = dir.join("a.svg");
        std::fs::write(&input, DRAWING).unwrap();
        let out = dir.join("out.gif");
        let mut opts = options();
        opts.dpr = 3;
        assert!(write_animation(&input, &out, &opts).is_err());
        opts = options();
        opts.fps = 60;
        assert!(write_animation(&input, &out, &opts).is_err());
        opts = options();
        opts.duration = 0.0;
        assert!(write_animation(&input, &out, &opts).is_err());
        opts = options();
        opts.width = Some(9000);
        let err = write_animation(&input, &out, &opts).unwrap_err();
        assert!(err.to_string().contains("8192"), "{err}");
        opts = options();
        opts.duration = 1e9;
        let err = write_animation(&input, &out, &opts).unwrap_err();
        assert!(err.to_string().contains("100000"), "{err}");
        let huge = dir.join("huge.svg");
        std::fs::write(
            &huge,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="9000" height="100"></svg>"#,
        )
        .unwrap();
        let err = write_animation(&huge, &out, &options()).unwrap_err();
        assert!(err.to_string().contains("8192"), "{err}");
        let err = write_animation(&dir.join("nope.svg"), &out, &options()).unwrap_err();
        assert!(err.to_string().contains("input not found"));
        std::fs::write(dir.join("a.png"), DRAWING).unwrap();
        let err = write_animation(&dir.join("a.png"), &out, &options()).unwrap_err();
        assert!(err.to_string().contains("must be .svg"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_setup_pauses_every_clock_at_zero() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        let Ok(exe) = discover(None) else {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        };
        let dir = scratch("clocks");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><style>@keyframes slide{from{transform:translateX(0)}to{transform:translateX(40px)}}rect{animation:slide 1s linear infinite}</style><rect width="10" height="10" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let page_svg = std::fs::read_to_string(&input).unwrap();
        let temp = write_page(&input, &page_svg, 80, 40, [255, 255, 255]).unwrap();
        let target = page::file_url(&temp.path).unwrap();
        let mut page = Page::open(&exe, 80, 40, 1, &target, [255, 255, 255]).unwrap();
        page.evaluate(
            &page::js_setup(false, Preset::None, None, 0.0, [255, 255, 255]),
            true,
        )
        .unwrap();
        let probe = "document.getAnimations().map((a) => a.currentTime + '/' + a.playState).join(',') + '#' + document.querySelector('svg').getCurrentTime()";
        let reply = page.evaluate(probe, false).unwrap();
        let text = reply
            .pointer("/result/value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(text, "0/paused#0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_page_cannot_read_a_local_file() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        let Ok(exe) = discover(None) else {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        };
        let dir = scratch("file-access");
        std::fs::write(dir.join("secret.txt"), "TOPSECRET").unwrap();
        let page_file = dir.join("probe.html");
        std::fs::write(&page_file, "<!doctype html><title>probe</title>").unwrap();
        let target = page::file_url(&page_file).unwrap();
        let secret = page::file_url(&dir.join("secret.txt")).unwrap();
        let mut page = Page::open(&exe, 40, 20, 1, &target, [255, 255, 255]).unwrap();
        let script = format!(
            "fetch('{secret}').then((reply) => reply.text()).then((text) => 'read:' + text).catch((err) => 'blocked:' + err.name)"
        );
        let reply = page.evaluate(&script, true).unwrap();
        let text = reply
            .pointer("/result/value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert_eq!(text, "blocked:TypeError", "the page read a local file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_captured_loop_moves_the_drawing() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("capture");
        let input = dir.join("drawing.svg");
        std::fs::write(&input, DRAWING).unwrap();
        let out = dir.join("out.gif");
        let rendered = write_animation(&input, &out, &options()).unwrap();
        assert_eq!(rendered.summary.frames, 4);
        assert_eq!(rendered.summary.total_ms, 1000);
        assert_eq!(rendered.first.size(), (80, 40));

        let frames = decoded(&out);
        assert_eq!(frames.len(), 4, "a merged hold would hide the motion");
        let red: Vec<f64> = frames
            .iter()
            .map(|frame| centre(frame, 80, [255, 0, 0]).expect("red block"))
            .collect();
        let blue: Vec<f64> = frames
            .iter()
            .map(|frame| centre(frame, 80, [0, 0, 255]).expect("blue block"))
            .collect();
        assert!(red[3] > red[0] + 20.0, "CSS move: {red:?}");
        assert!(blue[3] > blue[0] + 20.0, "SMIL move: {blue:?}");
        assert!(!dir.join("out.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_percentage_drawing_keeps_its_viewbox_ratio() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("percentage");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 400 200"><rect width="400" height="200" fill="#ffffff"/></svg>"##,
        )
        .unwrap();
        let out = dir.join("out.gif");
        let rendered = write_animation(&input, &out, &options()).unwrap();
        assert_eq!(
            (rendered.summary.width, rendered.summary.height),
            (400, 200)
        );
        let frames = decoded(&out);
        assert_eq!(frames.len(), 1, "a static drawing merges its frames");
        assert_eq!(rendered.summary.total_ms, 1000);
        assert!(!dir.join("out.tmp.gif").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A page with no intrinsic size, so the flags set the canvas.
    const PAGE: &str = r##"<!doctype html><html><head><meta charset="utf-8">
<style>html,body{margin:0;padding:0;background:#ffffff}#box{width:40px;height:40px;margin:20px;background:#c02020}</style>
</head><body><div id="box"></div></body></html>"##;

    #[test]
    fn an_html_page_is_captured_with_a_preset() {
        let _browser = BROWSER.lock().unwrap_or_else(|poison| poison.into_inner());
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("html-preset");
        let input = dir.join("page.html");
        std::fs::write(&input, PAGE).unwrap();
        let out = dir.join("out.gif");
        let mut opts = options();
        opts.width = Some(120);
        opts.height = Some(80);
        opts.duration = 1.0;
        opts.fps = 2;
        opts.animate = Preset::Pulse;
        opts.selector = Some("#box".to_string());
        let rendered = write_animation(&input, &out, &opts).unwrap();
        assert_eq!(rendered.summary.frames, 2);
        assert_eq!((rendered.summary.width, rendered.summary.height), (120, 80));
        let frames = decoded(&out);
        assert_eq!(frames.len(), 2, "the pulse must change the picture");
        assert_ne!(frames[0], frames[1]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
