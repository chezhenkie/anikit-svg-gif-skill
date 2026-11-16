//! Sampling an animated SVG into the canonical timeline (part 14).
//!
//! The browser resolves what moves; this module reads the five properties the
//! tween route can draw at the frame times that route will use, and writes the
//! JSON model of part 11. It parses no CSS and no SMIL: the values it records
//! are the ones the page computed.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use anikit_core::{Error, Result, check_canvas, check_frames, temp_path};
use anikit_time::LoopMode;
use anikit_tween::Transform;
use serde_json::{Map, Value as Json, json};

use crate::{Page, discover, page, write_page};

/// Everything the import route needs that is not an input or an output path.
#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// The sampled interval, in seconds.
    pub duration: f64,
    pub fps: u32,
    pub background: [u8; 3],
    pub loop_mode: LoopMode,
    pub browser: Option<PathBuf>,
}

/// What the route wrote, plus the properties it skipped.
#[derive(Clone, Debug)]
pub struct Imported {
    pub tracks: u32,
    pub total_ms: u32,
    pub width: u32,
    pub height: u32,
    pub skipped: Vec<String>,
}

/// Sample an animated SVG and write a scene file.
pub fn write_import(input: &Path, out: &Path, options: &ImportOptions) -> Result<Imported> {
    validate(input, out, options)?;
    let svg = std::fs::read_to_string(input)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", input.display())))?;
    let (width, height) = page::resolve_size(Some(&svg), options.width, options.height)?;
    if width < 8 || height < 8 {
        return Err(Error::invalid("width and height must be at least 8"));
    }
    check_canvas(width, height)?;
    let fps = options.fps;
    let count = ((options.duration * fps as f64).ceil() as usize).max(2);
    check_frames(count as u64)?;
    let delay = 1000 / fps;
    let total_ms = count as u64 * delay as u64;
    let total_ms =
        u32::try_from(total_ms).map_err(|_| Error::invalid("the sampled interval is too long"))?;
    let exe = discover(options.browser.as_deref())?;

    let temp = write_page(input, &svg, width, height, options.background)?;
    let target = page::file_url(&temp.path)?;
    let mut browser_page = Page::open(&exe, width, height, 1, &target, options.background)?;

    let reply = browser_page.evaluate(&js_setup(), true)?;
    let setup = parse_reply(&reply)?;
    let (targets, skipped) = parse_setup(&setup)?;
    if targets.is_empty() {
        return Err(Error::invalid("the drawing has no animation"));
    }

    let mut samples: Vec<Vec<Sample>> = (0..targets.len())
        .map(|_| Vec::with_capacity(count))
        .collect();
    for index in 0..count {
        let seconds = (index as f64 * delay as f64) / 1000.0;
        let reply = browser_page.evaluate(&js_sample(seconds), false)?;
        let text = value_text(&reply)?;
        let value: Json = serde_json::from_str(&text)
            .map_err(|err| Error::invalid(format!("the page sent an unreadable sample: {err}")))?;
        if let Some(error) = value.get("error").and_then(Json::as_str) {
            let id = value.get("id").and_then(Json::as_str).unwrap_or("");
            return Err(Error::invalid(match id {
                "" => format!("the page could not sample the drawing: {error}"),
                other => format!("the page could not sample {other}: {error}"),
            }));
        }
        for (position, meta) in targets.iter().enumerate() {
            let item = value.get(&meta.id).ok_or_else(|| {
                Error::invalid(format!("the page did not send a sample for {}", meta.id))
            })?;
            samples[position].push(parse_sample(item, &meta.id)?);
        }
    }
    drop(browser_page);
    drop(temp);

    let src = relative_src(input, out);
    let mut tracks = Vec::new();
    for (position, meta) in targets.iter().enumerate() {
        let base = parse_transform_list(&meta.transform).map_err(|err| {
            Error::invalid(format!(
                "the transform on {} cannot be read: {err}",
                meta.id
            ))
        })?;
        let inverse = invert(&base).ok_or_else(|| {
            Error::invalid(format!("the transform on {} cannot be inverted", meta.id))
        })?;
        let values = &samples[position];
        let origin = format!("{}#{}", src, meta.id);

        let deltas: Vec<Transform> = values
            .iter()
            .map(|sample| sample.transform.mul(&inverse))
            .collect();
        if transform_changes(&deltas) {
            tracks.push(transform_track(&meta.id, &origin, &deltas, delay, total_ms));
        }
        if number_changes(values.iter().map(|sample| sample.opacity)) {
            let keys = number_keys(values.iter().map(|sample| sample.opacity), delay, total_ms);
            tracks.push(track(&meta.id, "opacity", &origin, keys));
        }
        if colour_changes(values.iter().map(|sample| sample.fill)) {
            let colours: Option<Vec<[u8; 3]>> = values.iter().map(|sample| sample.fill).collect();
            let colours = colours.ok_or_else(|| {
                Error::invalid(format!(
                    "{} animates a fill that a track cannot carry",
                    meta.id
                ))
            })?;
            let keys = colour_keys(colours.into_iter(), delay, total_ms);
            tracks.push(track(&meta.id, "fill", &origin, keys));
        }
        if colour_changes(values.iter().map(|sample| sample.stroke)) {
            let colours: Option<Vec<[u8; 3]>> = values.iter().map(|sample| sample.stroke).collect();
            let colours = colours.ok_or_else(|| {
                Error::invalid(format!(
                    "{} animates a stroke that a track cannot carry",
                    meta.id
                ))
            })?;
            let keys = colour_keys(colours.into_iter(), delay, total_ms);
            tracks.push(track(&meta.id, "stroke", &origin, keys));
        }
        if number_changes(values.iter().map(|sample| sample.stroke_width)) {
            let keys = number_keys(
                values.iter().map(|sample| sample.stroke_width),
                delay,
                total_ms,
            );
            tracks.push(track(&meta.id, "stroke-width", &origin, keys));
        }
    }
    let tracks_count = u32::try_from(tracks.len()).unwrap_or(u32::MAX);
    if tracks_count == 0 {
        let detail = if skipped.is_empty() {
            String::new()
        } else {
            format!("; skipped {}", skipped.join(", "))
        };
        return Err(Error::invalid(format!(
            "the drawing has no importable animation{detail}"
        )));
    }

    let scene = json!({
        "version": 1,
        "canvas": {
            "w": width,
            "h": height,
            "bg": hex(options.background),
            "fps": fps,
        },
        "timing": {
            "loop": loop_name(options.loop_mode),
            "rate": 1.0,
        },
        "tracks": tracks,
        "layers": [],
    });
    let mut text = serde_json::to_string_pretty(&scene)
        .map_err(|err| Error::io(format!("cannot write the scene: {err}")))?;
    text.push('\n');
    write_and_verify(out, text.as_bytes())?;

    Ok(Imported {
        tracks: tracks_count,
        total_ms,
        width,
        height,
        skipped,
    })
}

fn validate(input: &Path, out: &Path, options: &ImportOptions) -> Result<()> {
    let extension = input
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());
    if extension.as_deref() != Some("svg") {
        return Err(Error::invalid("input must be .svg"));
    }
    if !input.is_file() {
        return Err(Error::invalid(format!(
            "input not found: {}",
            input.display()
        )));
    }
    if out
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
        != Some("json")
    {
        return Err(Error::invalid("output must be .json"));
    }
    if !options.duration.is_finite() || options.duration <= 0.0 {
        return Err(Error::invalid("duration must be greater than 0"));
    }
    if !exact_rate(options.fps) {
        return Err(Error::invalid(
            "fps must be one of: 1, 2, 4, 5, 10, 20, 25 or 50, so every delay is a multiple of 10 ms",
        ));
    }
    Ok(())
}

/// A rate whose delay is a whole unit on the 10 ms grid.
fn exact_rate(fps: u32) -> bool {
    fps > 0 && 1000 % fps == 0 && (1000 / fps).is_multiple_of(10)
}

fn loop_name(mode: LoopMode) -> &'static str {
    match mode {
        LoopMode::Once => "once",
        LoopMode::Cycle => "cycle",
        LoopMode::PingPong => "pingpong",
    }
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

/// One element the page reports as animated.
struct TargetMeta {
    id: String,
    transform: String,
}

/// One sampled state of one element.
struct Sample {
    transform: Transform,
    opacity: f64,
    fill: Option<[u8; 3]>,
    stroke: Option<[u8; 3]>,
    stroke_width: f64,
}

fn parse_reply(reply: &Json) -> Result<Json> {
    let text = value_text(reply)?;
    serde_json::from_str(&text)
        .map_err(|err| Error::invalid(format!("the page sent unreadable metadata: {err}")))
}

fn value_text(reply: &Json) -> Result<String> {
    reply
        .pointer("/result/value")
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid("the page sent no result"))
}

fn parse_setup(value: &Json) -> Result<(Vec<TargetMeta>, Vec<String>)> {
    if let Some(error) = value.get("error").and_then(Json::as_str) {
        if error == "missing ids" {
            let list = value
                .get("elements")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Json::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            return Err(Error::invalid(format!(
                "an animated element has no id: {list}"
            )));
        }
        return Err(Error::invalid(error));
    }
    let mut targets = Vec::new();
    let items = value
        .get("targets")
        .and_then(Json::as_array)
        .ok_or_else(|| Error::invalid("the page sent no animated elements"))?;
    for item in items {
        let id = item
            .get("id")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::invalid("the page sent an element without an id"))?;
        let transform = item
            .get("transform")
            .and_then(Json::as_str)
            .unwrap_or_default();
        targets.push(TargetMeta {
            id: id.to_string(),
            transform: transform.to_string(),
        });
    }
    let skipped = value
        .get("others")
        .and_then(Json::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((targets, skipped))
}

fn parse_sample(value: &Json, id: &str) -> Result<Sample> {
    let transform = value
        .get("transform")
        .and_then(Json::as_array)
        .ok_or_else(|| Error::invalid(format!("{id} sent no transform")))?;
    if transform.len() != 6 {
        return Err(Error::invalid(format!("{id} sent a short transform")));
    }
    let mut matrix = [0f64; 6];
    for (index, item) in transform.iter().enumerate() {
        matrix[index] = item
            .as_f64()
            .ok_or_else(|| Error::invalid(format!("{id} sent a transform that is not numeric")))?;
    }
    let opacity = value
        .get("opacity")
        .and_then(Json::as_str)
        .ok_or_else(|| Error::invalid(format!("{id} sent no opacity")))?
        .parse::<f64>()
        .map_err(|_| Error::invalid(format!("{id} sent an opacity that is not a number")))?;
    let fill = parse_colour(
        value
            .get("fill")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::invalid(format!("{id} sent no fill")))?,
    );
    let stroke = parse_colour(
        value
            .get("stroke")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::invalid(format!("{id} sent no stroke")))?,
    );
    let stroke_width = parse_length(
        value
            .get("strokeWidth")
            .and_then(Json::as_str)
            .ok_or_else(|| Error::invalid(format!("{id} sent no stroke width")))?,
        id,
    )?;
    Ok(Sample {
        transform: Transform {
            m11: matrix[0],
            m12: matrix[1],
            m21: matrix[2],
            m22: matrix[3],
            tx: matrix[4],
            ty: matrix[5],
        },
        opacity,
        fill,
        stroke,
        stroke_width,
    })
}

/// An opaque sRGB colour, or None for a paint a track cannot carry.
fn parse_colour(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("none") {
        return None;
    }
    if let Some(body) = text
        .strip_prefix("rgb(")
        .or_else(|| text.strip_prefix("rgba("))
        .and_then(|body| body.strip_suffix(')'))
    {
        let parts: Vec<&str> = body
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() != 3 && parts.len() != 4 {
            return None;
        }
        let mut rgb = [0u8; 3];
        for (index, part) in parts.iter().take(3).enumerate() {
            rgb[index] = parse_channel(part)?;
        }
        if let Some(alpha) = parts.get(3) {
            let alpha = if let Some(percent) = alpha.strip_suffix('%') {
                percent.parse::<f64>().ok()? / 100.0
            } else {
                alpha.parse::<f64>().ok()?
            };
            if (alpha - 1.0).abs() > 1e-6 {
                return None;
            }
        }
        return Some(rgb);
    }
    if let Some(body) = text.strip_prefix('#') {
        let hex = if body.len() == 3 {
            body.chars().flat_map(|ch| [ch, ch]).collect::<String>()
        } else {
            body.to_string()
        };
        if hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let mut rgb = [0u8; 3];
            for (index, byte) in rgb.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
            }
            return Some(rgb);
        }
    }
    None
}

fn parse_channel(text: &str) -> Option<u8> {
    if let Some(percent) = text.strip_suffix('%') {
        let value = percent.parse::<f64>().ok()?;
        return Some((value * 255.0 / 100.0).round().clamp(0.0, 255.0) as u8);
    }
    let value = text.parse::<f64>().ok()?;
    Some(value.round().clamp(0.0, 255.0) as u8)
}

fn parse_length(text: &str, id: &str) -> Result<f64> {
    let text = text.trim();
    let body = text.strip_suffix("px").unwrap_or(text);
    let value = body
        .parse::<f64>()
        .map_err(|_| Error::invalid(format!("{id} sent a length a track cannot carry: {text}")))?;
    if !value.is_finite() || value < 0.0 {
        return Err(Error::invalid(format!(
            "{id} sent a length a track cannot carry: {text}"
        )));
    }
    Ok(value)
}

fn transform_changes(values: &[Transform]) -> bool {
    let Some(first) = values.first() else {
        return false;
    };
    values.iter().any(|value| {
        (value.m11 - first.m11).abs() > 1e-6
            || (value.m12 - first.m12).abs() > 1e-6
            || (value.m21 - first.m21).abs() > 1e-6
            || (value.m22 - first.m22).abs() > 1e-6
            || (value.tx - first.tx).abs() > 1e-6
            || (value.ty - first.ty).abs() > 1e-6
    })
}

fn number_changes(values: impl Iterator<Item = f64>) -> bool {
    let mut first: Option<f64> = None;
    for value in values {
        match first {
            None => first = Some(value),
            Some(base) => {
                if (value - base).abs() > 1e-6 {
                    return true;
                }
            }
        }
    }
    false
}

fn colour_changes(values: impl Iterator<Item = Option<[u8; 3]>>) -> bool {
    let mut first: Option<Option<[u8; 3]>> = None;
    for value in values {
        match first {
            None => first = Some(value),
            Some(base) => {
                if value != base {
                    return true;
                }
            }
        }
    }
    false
}

fn round6(value: f64) -> f64 {
    let rounded = (value * 1e6).round() / 1e6;
    if rounded == 0.0 { 0.0 } else { rounded }
}

fn number_keys(values: impl Iterator<Item = f64>, delay: u32, total_ms: u32) -> Vec<Json> {
    let values: Vec<f64> = values.collect();
    let mut keys = Vec::with_capacity(values.len() + 1);
    for (index, value) in values.iter().enumerate() {
        let mut key = Map::new();
        key.insert("t".to_string(), json!(index as u32 * delay));
        key.insert("easing".to_string(), json!("linear"));
        key.insert("value".to_string(), json!(round6(*value)));
        keys.push(Json::Object(key));
    }
    if let Some(last) = values.last() {
        let mut key = Map::new();
        key.insert("t".to_string(), json!(total_ms));
        key.insert("value".to_string(), json!(round6(*last)));
        keys.push(Json::Object(key));
    }
    keys
}

fn colour_keys(values: impl Iterator<Item = [u8; 3]>, delay: u32, total_ms: u32) -> Vec<Json> {
    let values: Vec<[u8; 3]> = values.collect();
    let mut keys = Vec::with_capacity(values.len() + 1);
    for (index, value) in values.iter().enumerate() {
        let mut key = Map::new();
        key.insert("t".to_string(), json!(index as u32 * delay));
        key.insert("easing".to_string(), json!("linear"));
        key.insert("value".to_string(), json!(hex(*value)));
        keys.push(Json::Object(key));
    }
    if let Some(last) = values.last() {
        let mut key = Map::new();
        key.insert("t".to_string(), json!(total_ms));
        key.insert("value".to_string(), json!(hex(*last)));
        keys.push(Json::Object(key));
    }
    keys
}

fn transform_track(id: &str, src: &str, values: &[Transform], delay: u32, total_ms: u32) -> Json {
    let mut keys = Vec::with_capacity(values.len() + 1);
    let mut push = |time: u32, value: &Transform, easing: bool| {
        let decomposed = value.decompose();
        let mut key = Map::new();
        key.insert("t".to_string(), json!(time));
        if easing {
            key.insert("easing".to_string(), json!("linear"));
        }
        key.insert(
            "translate".to_string(),
            json!([round6(decomposed.tx), round6(decomposed.ty)]),
        );
        key.insert(
            "rotate".to_string(),
            json!(round6(decomposed.rotation.to_degrees())),
        );
        key.insert(
            "scale".to_string(),
            json!([round6(decomposed.scale_x), round6(decomposed.scale_y)]),
        );
        key.insert(
            "skew".to_string(),
            json!(round6(decomposed.skew.to_degrees())),
        );
        keys.push(Json::Object(key));
    };
    for (index, value) in values.iter().enumerate() {
        push(index as u32 * delay, value, true);
    }
    if let Some(last) = values.last() {
        push(total_ms, last, false);
    }
    json!({
        "id": format!("{id}-transform"),
        "kind": "svg.transform",
        "src": src,
        "keys": keys,
    })
}

fn track(id: &str, property: &str, src: &str, keys: Vec<Json>) -> Json {
    json!({
        "id": format!("{id}-{property}"),
        "kind": format!("svg.{property}"),
        "src": src,
        "keys": keys,
    })
}

/// Invert a 2D affine transform, or report that it is singular.
fn invert(value: &Transform) -> Option<Transform> {
    let determinant = value.m11 * value.m22 - value.m12 * value.m21;
    if determinant.abs() < 1e-12 {
        return None;
    }
    let m11 = value.m22 / determinant;
    let m21 = -value.m21 / determinant;
    let m12 = -value.m12 / determinant;
    let m22 = value.m11 / determinant;
    Some(Transform {
        m11,
        m12,
        m21,
        m22,
        tx: -(m11 * value.tx + m21 * value.ty),
        ty: -(m12 * value.tx + m22 * value.ty),
    })
}

/// Parse an SVG transform list: matrix, translate, scale, rotate, skewX, skewY.
pub(crate) fn parse_transform_list(text: &str) -> Result<Transform> {
    let mut result = Transform::IDENTITY;
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    while pos < bytes.len() {
        while pos < bytes.len() && (bytes[pos].is_ascii_whitespace() || bytes[pos] == b',') {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_alphabetic() {
            pos += 1;
        }
        if start == pos {
            return Err(Error::invalid(format!(
                "unexpected text in transform: {text}"
            )));
        }
        let name = text[start..pos].to_ascii_lowercase();
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'(' {
            return Err(Error::invalid(format!("{name} has no argument list")));
        }
        pos += 1;
        let values_start = pos;
        while pos < bytes.len() && bytes[pos] != b')' {
            pos += 1;
        }
        if pos >= bytes.len() {
            return Err(Error::invalid(format!("{name} is not closed")));
        }
        let body = &text[values_start..pos];
        pos += 1;
        let values = parse_numbers(body)?;
        let step = match name.as_str() {
            "matrix" => {
                if values.len() != 6 {
                    return Err(Error::invalid("matrix needs six numbers"));
                }
                Transform {
                    m11: values[0],
                    m12: values[1],
                    m21: values[2],
                    m22: values[3],
                    tx: values[4],
                    ty: values[5],
                }
            }
            "translate" => match values.as_slice() {
                [x] => Transform::translate(*x, 0.0),
                [x, y] => Transform::translate(*x, *y),
                _ => return Err(Error::invalid("translate needs one or two numbers")),
            },
            "scale" => match values.as_slice() {
                [x] => Transform::scale(*x, *x),
                [x, y] => Transform::scale(*x, *y),
                _ => return Err(Error::invalid("scale needs one or two numbers")),
            },
            "rotate" => match values.as_slice() {
                [degrees] => Transform::rotate(degrees.to_radians()),
                [degrees, cx, cy] => Transform::translate(*cx, *cy)
                    .mul(&Transform::rotate(degrees.to_radians()))
                    .mul(&Transform::translate(-*cx, -*cy)),
                _ => return Err(Error::invalid("rotate needs one or three numbers")),
            },
            "skewx" => match values.as_slice() {
                [degrees] => Transform {
                    m21: degrees.to_radians().tan(),
                    ..Transform::IDENTITY
                },
                _ => return Err(Error::invalid("skewX needs one number")),
            },
            "skewy" => match values.as_slice() {
                [degrees] => Transform {
                    m12: degrees.to_radians().tan(),
                    ..Transform::IDENTITY
                },
                _ => return Err(Error::invalid("skewY needs one number")),
            },
            other => return Err(Error::invalid(format!("unknown transform {other}"))),
        };
        result = result.mul(&step);
    }
    Ok(result)
}

fn parse_numbers(text: &str) -> Result<Vec<f64>> {
    let mut values = Vec::new();
    for part in text.split(|ch: char| ch == ',' || ch.is_whitespace()) {
        if part.is_empty() {
            continue;
        }
        let value = part
            .parse::<f64>()
            .map_err(|_| Error::invalid(format!("{part} is not a number")))?;
        if !value.is_finite() {
            return Err(Error::invalid(format!("{part} is not finite")));
        }
        values.push(value);
    }
    Ok(values)
}

/// The drawing path relative to the scene, with forward slashes.
fn relative_src(input: &Path, out: &Path) -> String {
    let Ok(input) = std::path::absolute(input) else {
        return slash_path(input);
    };
    let Ok(out) = std::path::absolute(out) else {
        return slash_path(&input);
    };
    let base = out.parent().unwrap_or(Path::new(""));
    let target: Vec<Component> = input.components().collect();
    let base_parts: Vec<Component> = base.components().collect();
    let mut common = 0;
    while common < target.len() && common < base_parts.len() && target[common] == base_parts[common]
    {
        common += 1;
    }
    if common == 0 {
        return slash_path(&input);
    }
    let mut out_text = String::new();
    for _ in common..base_parts.len() {
        out_text.push_str("../");
    }
    for component in &target[common..] {
        out_text.push_str(&component.as_os_str().to_string_lossy());
        out_text.push('/');
    }
    out_text.pop();
    out_text
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Write the temp file, read it back, then move it into place.
fn write_and_verify(out: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_path(out);
    let result = (|| -> Result<()> {
        std::fs::write(&temp, bytes)?;
        let back = std::fs::read(&temp)?;
        if back != bytes {
            return Err(Error::verify("the scene read back differently"));
        }
        std::fs::rename(&temp, out)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn js_setup() -> String {
    r#"(async () => {
  if (document.fonts && document.fonts.ready) { await document.fonts.ready; }
  await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));
  document.getAnimations().forEach((a) => { try { a.pause(); } catch (e) {} });
  const svg = document.querySelector('svg');
  if (svg && svg.pauseAnimations) { svg.pauseAnimations(); }
  const animated = new Set();
  const isSvg = (el) => el && el.nodeType === 1 && el.namespaceURI === 'http://www.w3.org/2000/svg';
  document.getAnimations().forEach((a) => {
    const target = a.effect && a.effect.target;
    if (isSvg(target)) { animated.add(target); }
  });
  document.querySelectorAll('animate, animateTransform, animateMotion, animateColor, set').forEach((a) => {
    const target = a.parentElement;
    if (isSvg(target)) { animated.add(target); }
  });
  const list = [...animated];
  const missing = list.filter((el) => !el.id).map((el) => el.tagName.toLowerCase());
  if (missing.length) { return JSON.stringify({error: 'missing ids', elements: missing}); }
  if (svg && list.includes(svg)) { return JSON.stringify({error: 'the root svg is animated'}); }
  const supported = new Set(['transform', 'opacity', 'fill', 'stroke', 'stroke-width']);
  const others = new Set();
  document.getAnimations().forEach((a) => {
    if (!a.effect || !a.effect.getKeyframes) { return; }
    a.effect.getKeyframes().forEach((keyframe) => {
      Object.keys(keyframe).forEach((key) => {
        if (key === 'offset' || key === 'computedOffset' || key === 'easing' || key === 'composite') { return; }
        const name = key === 'strokeWidth' ? 'stroke-width' : key;
        if (!supported.has(name)) { others.add(name); }
      });
    });
  });
  document.querySelectorAll('animate, animateTransform, animateMotion, animateColor, set').forEach((a) => {
    const name = (a.getAttribute('attributeName') || '').trim();
    if (name && !supported.has(name)) { others.add(name); }
    if (a.localName === 'animateColor') { others.add('animateColor'); }
  });
  const targets = list.map((el) => ({id: el.id, transform: el.getAttribute('transform') || '', motion: Array.from(el.children).some((child) => child.localName === 'animateMotion')}));
  window.__anikitImport = {targets};
  return JSON.stringify({targets, others: [...others]});
})()"#
        .to_string()
}

fn js_sample(seconds: f64) -> String {
    let setup = r#"(() => {
  const t = "#;
    let mut text = String::with_capacity(2048);
    text.push_str(setup);
    let _ = write!(text, "{seconds:?}");
    text.push_str(
        r#";
  document.getAnimations().forEach((a) => { try { a.currentTime = t * 1000; } catch (e) {} });
  const svg = document.querySelector('svg');
  if (svg && svg.setCurrentTime) { svg.setCurrentTime(t); }
  const out = {};
  for (const target of window.__anikitImport.targets) {
    const el = document.getElementById(target.id);
    if (!el) { return JSON.stringify({error: 'element not found', id: target.id}); }
    const style = getComputedStyle(el);
    const attr = el.transform && el.transform.baseVal ? el.transform.baseVal.consolidate() : null;
    const base = attr ? [attr.matrix.a, attr.matrix.b, attr.matrix.c, attr.matrix.d, attr.matrix.e, attr.matrix.f] : [1, 0, 0, 1, 0, 0];
    let matrix = base;
    const css = style.transform;
    if (css && css !== 'none') {
      const m = new DOMMatrix(css);
      const same = Math.abs(m.a - base[0]) < 1e-6 && Math.abs(m.b - base[1]) < 1e-6 && Math.abs(m.c - base[2]) < 1e-6 && Math.abs(m.d - base[3]) < 1e-6 && Math.abs(m.e - base[4]) < 1e-6 && Math.abs(m.f - base[5]) < 1e-6;
      if (!same) {
        const parts = style.transformOrigin.split(/\s+/);
        const x = parseFloat(parts[0] || '0');
        const y = parseFloat(parts[1] || '0');
        if (!isFinite(x) || !isFinite(y) || Math.abs(x) > 1e-6 || Math.abs(y) > 1e-6) {
          return JSON.stringify({error: 'transform origin is not 0 0', id: target.id, origin: style.transformOrigin});
        }
        matrix = [m.a, m.b, m.c, m.d, m.e, m.f];
      }
    }
    if (target.motion) {
      // animateMotion is a supplemental transform, so the element's own
      // transform does not carry it. The local matrix relative to the parent
      // does, and the wrapper applies it outside the authored transform.
      const own = el.getCTM ? el.getCTM() : null;
      const parent = el.parentElement;
      const above = parent && parent.getCTM ? parent.getCTM() : null;
      if (own) {
        const m = above ? above.inverse().multiply(own) : own;
        matrix = [m.a, m.b, m.c, m.d, m.e, m.f];
      }
    }
    out[target.id] = {
      transform: matrix,
      opacity: style.opacity,
      fill: style.fill,
      stroke: style.stroke,
      strokeWidth: style.strokeWidth,
    };
  }
  return JSON.stringify(out);
})()"#,
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/import")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options() -> ImportOptions {
        ImportOptions {
            width: None,
            height: None,
            duration: 1.0,
            fps: 10,
            background: [255, 255, 255],
            loop_mode: LoopMode::Cycle,
            browser: None,
        }
    }

    fn scene_of(path: &Path) -> Json {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn find_track<'a>(scene: &'a Json, id: &str) -> &'a Json {
        scene["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|track| track["id"] == id)
            .unwrap_or_else(|| panic!("no track {id}"))
    }

    fn browser_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::test_lock::BROWSER
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    const CSS_DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
<style>
@keyframes slide { from { transform: translateX(0px); } to { transform: translateX(40px); } }
@keyframes fade { from { opacity: 1; } to { opacity: 0.5; } }
#box { animation: slide 1s linear infinite, fade 1s linear infinite; }
</style>
<rect id="box" x="0" y="0" width="20" height="20" fill="#ff0000"/>
</svg>"##;

    /// A drawing whose animation starts away from the element's own position, so a
    /// replay of the source animation lands 40 px to the right of the sampled spot.
    const OFFSET_DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40" viewBox="0 0 120 40">
<style>
@keyframes slide { from { transform: translateX(40px); } to { transform: translateX(60px); } }
#box { animation: slide 1s linear infinite; }
</style>
<rect id="box" x="0" y="0" width="20" height="20" fill="#ff0000"/>
</svg>"##;

    #[test]
    fn a_css_drawing_imports_its_motion() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("css");
        let input = dir.join("art.svg");
        std::fs::write(&input, CSS_DRAWING).unwrap();
        let out = dir.join("scene.json");
        let imported = write_import(&input, &out, &options()).unwrap();
        assert_eq!(imported.tracks, 2);
        assert_eq!(imported.total_ms, 1000);
        assert_eq!((imported.width, imported.height), (80, 40));

        let scene = scene_of(&out);
        assert_eq!(scene["canvas"]["fps"], 10);
        assert_eq!(scene["timing"]["loop"], "cycle");
        let transform = find_track(&scene, "box-transform");
        let keys = transform["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 11);
        assert_eq!(keys[0]["t"], 0);
        assert_eq!(keys[0]["translate"], json!([0.0, 0.0]));
        assert_eq!(keys[9]["t"], 900);
        assert_eq!(keys[9]["translate"], json!([36.0, 0.0]));
        assert_eq!(keys[10]["t"], 1000);
        let opacity = find_track(&scene, "box-opacity");
        let keys = opacity["keys"].as_array().unwrap();
        assert_eq!(keys[9]["value"], json!(0.55));
        assert!(transform["src"].as_str().unwrap().ends_with("art.svg#box"));
        assert!(!dir.join("scene.tmp.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_twenty_five_fps_import_closes_on_the_duration() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("fps-25");
        let input = dir.join("art.svg");
        std::fs::write(&input, CSS_DRAWING).unwrap();
        let out = dir.join("scene.json");
        let options = ImportOptions {
            fps: 25,
            ..options()
        };
        let imported = write_import(&input, &out, &options).unwrap();
        assert_eq!(imported.total_ms, 1000);

        let scene = scene_of(&out);
        assert_eq!(scene["canvas"]["fps"], 25);
        let transform = find_track(&scene, "box-transform");
        let keys = transform["keys"].as_array().unwrap();
        assert_eq!(keys.len(), 26);
        assert_eq!(keys[1]["t"], 40);
        assert_eq!(keys[24]["t"], 960);
        assert_eq!(keys[25]["t"], 1000);
        let closing = keys[25]["translate"][0].as_f64().unwrap();
        assert!((closing - 38.4).abs() < 1e-6, "closing value {closing}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const MOTION_DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
<rect id="box" x="0" y="0" width="10" height="10" fill="#ff0000">
  <animateMotion path="M 0 0 L 40 0" dur="1s" fill="freeze"/>
</rect>
</svg>"##;

    #[test]
    fn an_animate_motion_drawing_imports_its_transform() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("motion");
        let input = dir.join("art.svg");
        std::fs::write(&input, MOTION_DRAWING).unwrap();
        let out = dir.join("scene.json");
        let imported = write_import(&input, &out, &options()).unwrap();
        assert_eq!(imported.tracks, 1);

        let scene = scene_of(&out);
        let transform = find_track(&scene, "box-transform");
        let keys = transform["keys"].as_array().unwrap();
        assert_eq!(keys[9]["t"], 900);
        let x = keys[9]["translate"][0].as_f64().unwrap();
        assert!((x - 36.0).abs() < 2.0, "animateMotion x {x}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const COLOUR_DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
<rect id="box" x="0" y="0" width="10" height="10" fill="#ff0000">
  <animateColor attributeName="fill" values="#ff0000;#0000ff" dur="1s" fill="freeze"/>
  <animate attributeName="opacity" values="1;0.5" dur="1s" fill="freeze"/>
</rect>
</svg>"##;

    #[test]
    fn an_animate_color_drawing_reports_the_unsupported_colour() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("colour");
        let input = dir.join("art.svg");
        std::fs::write(&input, COLOUR_DRAWING).unwrap();
        let out = dir.join("scene.json");
        let imported = write_import(&input, &out, &options()).unwrap();
        // Chromium does not run animateColor, so the route reports it instead
        // of claiming a colour track it cannot sample.
        assert!(
            imported
                .skipped
                .iter()
                .any(|name| name.contains("animateColor")),
            "skipped: {:?}",
            imported.skipped
        );
        assert_eq!(imported.tracks, 1);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("svg.opacity"), "{text}");
        assert!(!text.contains("svg.fill"), "no fill track: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    const SMIL_DRAWING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40" viewBox="0 0 80 40">
<rect id="box" transform="translate(10 5)" x="0" y="0" width="20" height="20" fill="#00ff00">
  <animate attributeName="opacity" values="1;0.25" dur="1s" fill="freeze"/>
  <animateTransform attributeName="transform" type="translate" values="0 0;40 0" dur="1s" fill="freeze" additive="replace"/>
</rect>
</svg>"##;

    #[test]
    fn a_smil_drawing_imports_its_motion() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("smil");
        let input = dir.join("art.svg");
        std::fs::write(&input, SMIL_DRAWING).unwrap();
        let out = dir.join("scene.json");
        let imported = write_import(&input, &out, &options()).unwrap();
        assert_eq!(imported.tracks, 2);

        let scene = scene_of(&out);
        let transform = find_track(&scene, "box-transform");
        let keys = transform["keys"].as_array().unwrap();
        // The drawing keeps its static translate(10 5), so the wrapper carries
        // the difference: the source renders translate(36 0) at 900 ms.
        assert_eq!(keys[9]["translate"], json!([26.0, -5.0]));
        let opacity = find_track(&scene, "box-opacity");
        let keys = opacity["keys"].as_array().unwrap();
        let value = keys[9]["value"].as_f64().unwrap();
        assert!((value - 0.325).abs() < 1e-3, "{value}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_drawing_without_animation_is_refused() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("still");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect id="box" width="4" height="4"/></svg>"#,
        )
        .unwrap();
        let out = dir.join("scene.json");
        let err = write_import(&input, &out, &options()).unwrap_err();
        assert!(err.to_string().contains("no animation"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_animated_element_without_an_id_is_refused() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("no-id");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
<style>@keyframes move { from { transform: translateX(0); } to { transform: translateX(10px); } } rect { animation: move 1s linear infinite; }</style>
<rect width="4" height="4"/></svg>"##,
        )
        .unwrap();
        let out = dir.join("scene.json");
        let err = write_import(&input, &out, &options()).unwrap_err();
        assert!(err.to_string().contains("no id"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_non_zero_transform_origin_is_refused() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("origin");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
<style>@keyframes turn { from { transform: rotate(0deg); } to { transform: rotate(90deg); } } #box { transform-origin: center; animation: turn 1s linear infinite; }</style>
<rect id="box" width="10" height="10"/></svg>"##,
        )
        .unwrap();
        let out = dir.join("scene.json");
        let err = write_import(&input, &out, &options()).unwrap_err();
        assert!(err.to_string().contains("transform origin"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_exported_scene_imports_back() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("roundtrip");
        let art = dir.join("art.svg");
        std::fs::write(
            &art,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><rect id="box" x="0" y="0" width="20" height="20" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let source = dir.join("source.json");
        std::fs::write(
            &source,
            r##"{
  "version": 1,
  "canvas": {"w": 80, "h": 40, "bg": "#ffffff", "fps": 10},
  "timing": {"loop": "cycle", "rate": 1.0},
  "tracks": [
    {"id": "move", "kind": "svg.transform", "src": "art.svg#box",
     "keys": [
       {"t": 0, "translate": [0, 0]},
       {"t": 1000, "translate": [40, 0]}
     ]}
  ],
  "layers": []
}"##,
        )
        .unwrap();
        let exported = dir.join("exported.svg");
        let rendered = crate::write_tween(
            &source,
            &exported,
            &crate::TweenOptions {
                colors: 255,
                dither: anikit_core::Dither::None,
                loop_count: 0,
                browser: None,
            },
        )
        .unwrap();
        assert_eq!(rendered.format, anikit_core::OutputFormat::Svg);

        let imported_path = dir.join("imported.json");
        let imported = write_import(&exported, &imported_path, &options()).unwrap();
        assert_eq!(imported.total_ms, 1000);
        assert_eq!(imported.tracks, 1);
        let scene = scene_of(&imported_path);
        let transform = find_track(&scene, "box-transform");
        let keys = transform["keys"].as_array().unwrap();
        assert_eq!(keys[0]["translate"], json!([0.0, 0.0]));
        assert_eq!(keys[9]["translate"], json!([36.0, 0.0]));
        assert_eq!(keys[10]["t"], 1000);
        assert!(
            transform["src"]
                .as_str()
                .unwrap()
                .ends_with("exported.svg#box")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_animation_outside_the_five_is_not_imported() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("outside");
        let input = dir.join("art.svg");
        std::fs::write(
            &input,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">
<rect id="box" width="4" height="4"><animate attributeName="x" values="0;10" dur="1s" fill="freeze"/></rect>
</svg>"##,
        )
        .unwrap();
        let out = dir.join("scene.json");
        let err = write_import(&input, &out, &options()).unwrap_err();
        assert!(err.to_string().contains("no importable animation"));
        assert!(err.to_string().contains("x"), "{err}");
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_imported_scene_renders_through_the_tween_route() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("render");
        let input = dir.join("art.svg");
        std::fs::write(&input, CSS_DRAWING).unwrap();
        let scene_path = dir.join("scene.json");
        write_import(&input, &scene_path, &options()).unwrap();
        let out = dir.join("out.gif");
        let rendered = crate::write_tween(
            &scene_path,
            &out,
            &crate::TweenOptions {
                colors: 255,
                dither: anikit_core::Dither::None,
                loop_count: 0,
                browser: None,
            },
        )
        .unwrap();
        assert_eq!(rendered.summary.frames, 10);
        let info = anikit_core::inspect_file(&out).unwrap();
        assert_eq!(info.total_ms, 1000);

        let bytes = std::fs::read(&out).unwrap();
        let mut decoder = gif::DecodeOptions::new();
        decoder.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = decoder.read_info(&bytes[..]).unwrap();
        let screen_w = decoder.width() as usize;
        let screen_h = decoder.height() as usize;
        let mut canvas = vec![[0u8; 4]; screen_w * screen_h];
        let mut previous: Option<(usize, usize, usize, usize, gif::DisposalMethod)> = None;
        let mut centres = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            if let Some((left, top, width, height, gif::DisposalMethod::Background)) = previous {
                for y in top..top + height {
                    for x in left..left + width {
                        canvas[y * screen_w + x] = [0, 0, 0, 0];
                    }
                }
            }
            let left = frame.left as usize;
            let top = frame.top as usize;
            let width = frame.width as usize;
            let height = frame.height as usize;
            for y in 0..height {
                for x in 0..width {
                    let pixel = &frame.buffer[(y * width + x) * 4..(y * width + x) * 4 + 4];
                    if pixel[3] > 0 {
                        canvas[(top + y) * screen_w + left + x] =
                            [pixel[0], pixel[1], pixel[2], pixel[3]];
                    }
                }
            }
            previous = Some((left, top, width, height, frame.dispose));
            let mut sum = 0.0f64;
            let mut count = 0.0f64;
            for (index, pixel) in canvas.iter().enumerate() {
                // The fade track makes the box partially transparent over the matte,
                // so a red pixel is red dominant rather than a fixed channel value.
                let red = pixel[0] > 200
                    && i32::from(pixel[1]) + 80 < i32::from(pixel[0])
                    && i32::from(pixel[2]) + 80 < i32::from(pixel[0]);
                if red {
                    sum += (index % screen_w) as f64;
                    count += 1.0;
                }
            }
            centres.push(sum / count.max(1.0));
        }
        assert!(centres.len() >= 2);
        let last = *centres.last().unwrap();
        assert!(last > centres[0] + 20.0, "the box should move: {centres:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rendered_import_does_not_replay_the_source_animation() {
        let _browser = browser_lock();
        if discover(None).is_err() {
            eprintln!("skipping: no Chromium based browser installed");
            return;
        }
        let dir = scratch("no-replay");
        let input = dir.join("art.svg");
        std::fs::write(&input, OFFSET_DRAWING).unwrap();
        let scene_path = dir.join("scene.json");
        write_import(&input, &scene_path, &options()).unwrap();
        let out = dir.join("out.gif");
        crate::write_tween(
            &scene_path,
            &out,
            &crate::TweenOptions {
                colors: 255,
                dither: anikit_core::Dither::None,
                loop_count: 0,
                browser: None,
            },
        )
        .unwrap();

        let bytes = std::fs::read(&out).unwrap();
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(&bytes[..]).unwrap();
        let frame = decoder.read_next_frame().unwrap().unwrap();
        let width = frame.width as usize;
        let mut left: Option<i32> = None;
        for y in 0..frame.height as usize {
            for x in 0..width {
                let at = (y * width + x) * 4;
                let pixel = &frame.buffer[at..at + 4];
                if pixel[3] > 0 && pixel[0] > 200 && pixel[1] < 70 && pixel[2] < 70 {
                    let at = x as i32 + frame.left as i32;
                    left = Some(left.map_or(at, |best| best.min(at)));
                }
            }
        }
        let left = left.expect("the box should be drawn");
        assert!(
            (left - 40).abs() <= 2,
            "the box starts at x={left}, expected the sampled 40, so the source animation must not replay"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transform_lists_compose_in_order() {
        let value = parse_transform_list("translate(10 20) scale(2)").unwrap();
        assert_eq!(value.apply(1.0, 1.0), [12.0, 22.0]);
        let rotated = parse_transform_list("rotate(90)").unwrap();
        let [x, y] = rotated.apply(1.0, 0.0);
        assert!(x.abs() < 1e-9 && (y - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_matrix_round_trips_through_decomposition() {
        let value = Transform {
            m11: 0.8,
            m12: 0.4,
            m21: -0.2,
            m22: 1.3,
            tx: 12.0,
            ty: -4.0,
        };
        let rebuilt = Transform::from_decomposed(&value.decompose());
        for (a, b) in [
            (value.m11, rebuilt.m11),
            (value.m12, rebuilt.m12),
            (value.m21, rebuilt.m21),
            (value.m22, rebuilt.m22),
            (value.tx, rebuilt.tx),
            (value.ty, rebuilt.ty),
        ] {
            assert!((a - b).abs() < 1e-9, "{a} != {b}");
        }
    }

    #[test]
    fn the_inverse_undoes_a_transform() {
        let value = Transform::translate(10.0, 20.0)
            .mul(&Transform::rotate(0.3))
            .mul(&Transform::scale(2.0, 0.5));
        let inverse = invert(&value).unwrap();
        let product = value.mul(&inverse);
        for (a, b) in [
            (product.m11, 1.0),
            (product.m12, 0.0),
            (product.m21, 0.0),
            (product.m22, 1.0),
            (product.tx, 0.0),
            (product.ty, 0.0),
        ] {
            assert!((a - b).abs() < 1e-9, "{a} != {b}");
        }
    }

    #[test]
    fn colours_and_lengths_parse() {
        assert_eq!(parse_colour("rgb(255, 0, 16)"), Some([255, 0, 16]));
        assert_eq!(parse_colour("#0a0d12"), Some([10, 13, 18]));
        assert_eq!(parse_colour("rgb(100%, 0%, 0%)"), Some([255, 0, 0]));
        assert_eq!(parse_colour("none"), None);
        assert_eq!(parse_colour("rgba(1, 2, 3, 0.5)"), None);
        assert_eq!(parse_colour("rgba(1, 2, 3, 1)"), Some([1, 2, 3]));
        assert_eq!(parse_length("2.5px", "x").unwrap(), 2.5);
        assert!(parse_length("50%", "x").is_err());
    }

    #[test]
    fn a_relative_source_path_uses_dot_dot() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/import")
            .join("paths");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("drawings")).unwrap();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let input = dir.join("drawings/a.svg");
        std::fs::write(&input, "<svg/>").unwrap();
        let src = relative_src(&input, &dir.join("out/scene.json"));
        assert_eq!(src, "../drawings/a.svg");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_options_are_rejected_before_a_browser_starts() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/import")
            .join("bad-options");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("a.svg");
        std::fs::write(&input, "<svg/>").unwrap();
        let out = dir.join("scene.json");
        let options = ImportOptions {
            width: None,
            height: None,
            duration: 1.0,
            fps: 15,
            background: [255, 255, 255],
            loop_mode: LoopMode::Cycle,
            browser: None,
        };
        let err = write_import(&input, &out, &options).unwrap_err();
        assert!(err.to_string().contains("1, 2, 4, 5, 10, 20, 25 or 50"));
        let wide = ImportOptions {
            width: Some(9000),
            fps: 10,
            ..options.clone()
        };
        let err = write_import(&input, &out, &wide).unwrap_err();
        assert!(err.to_string().contains("8192"), "{err}");
        let long = ImportOptions {
            duration: 1e9,
            fps: 10,
            ..options.clone()
        };
        let err = write_import(&input, &out, &long).unwrap_err();
        assert!(err.to_string().contains("100000"), "{err}");
        for fps in [8, 40] {
            let options = ImportOptions {
                fps,
                ..options.clone()
            };
            let err = write_import(&input, &out, &options).unwrap_err();
            assert!(
                err.to_string().contains("multiple of 10 ms"),
                "fps {fps}: {err}"
            );
        }
        let options = ImportOptions { fps: 10, ..options };
        let err = write_import(&input, &dir.join("scene.txt"), &options).unwrap_err();
        assert!(err.to_string().contains(".json"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
