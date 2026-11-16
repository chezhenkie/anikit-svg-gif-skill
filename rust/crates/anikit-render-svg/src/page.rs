//! Page construction and the scripts that seek animations.
//!
//! An SVG is wrapped in a page at the requested viewport; an HTML page is
//! opened as it is and only gains the background style. Presets add one
//! keyframe rule and one class, which is what the frozen Python does.

use anikit_core::{Error, Result};

/// The motion a preset applies to the whole drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Preset {
    #[default]
    None,
    Rotate,
    Pulse,
    Fade,
}

impl Preset {
    /// The preset a command line name stands for.
    pub fn parse(name: &str) -> Result<Preset> {
        match name {
            "none" => Ok(Preset::None),
            "rotate" => Ok(Preset::Rotate),
            "pulse" => Ok(Preset::Pulse),
            "fade" => Ok(Preset::Fade),
            other => Err(Error::invalid(format!(
                "animate must be one of: fade, none, pulse, rotate, got {other}"
            ))),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Preset::None => "none",
            Preset::Rotate => "rotate",
            Preset::Pulse => "pulse",
            Preset::Fade => "fade",
        }
    }
}

/// The size an SVG declares, in CSS pixels, with percentages unresolved.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SvgSize {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub view_width: Option<f64>,
    pub view_height: Option<f64>,
    pub width_percent: bool,
    pub height_percent: bool,
}

/// Read width, height and viewBox from the root svg element.
pub fn svg_size(text: &str) -> Result<SvgSize> {
    let lower = text.to_ascii_lowercase();
    let start = lower
        .find("<svg")
        .ok_or_else(|| Error::invalid("no <svg> element found"))?;
    let bytes = text.as_bytes();
    let mut end = start + 4;
    let mut quote = 0u8;
    while end < bytes.len() {
        let byte = bytes[end];
        if quote != 0 {
            if byte == quote {
                quote = 0;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = byte;
        } else if byte == b'>' {
            break;
        }
        end += 1;
    }
    let tag = &text[start..end.min(text.len())];
    let mut size = SvgSize::default();
    for (name, value) in attributes(tag) {
        match name.as_str() {
            "width" => {
                if let Some((number, percent)) = length(&value) {
                    size.width = Some(number);
                    size.width_percent = percent;
                }
            }
            "height" => {
                if let Some((number, percent)) = length(&value) {
                    size.height = Some(number);
                    size.height_percent = percent;
                }
            }
            "viewbox" => {
                let parts: Vec<&str> = value
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|part| !part.is_empty())
                    .collect();
                if parts.len() == 4 {
                    size.view_width = parts[2].parse().ok();
                    size.view_height = parts[3].parse().ok();
                }
            }
            _ => {}
        }
    }
    Ok(size)
}

/// Name and value of every attribute in one start tag.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte.is_ascii_alphabetic() || byte == b'_' || byte == b':' {
            let name_start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'_' | b':' | b'-' | b'.'))
            {
                i += 1;
            }
            let name = tag[name_start..i].to_ascii_lowercase();
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'=' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let quote = bytes[i];
                    i += 1;
                    let value_start = i;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    out.push((name, tag[value_start..i].to_string()));
                    i += 1;
                } else {
                    let value_start = i;
                    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                        i += 1;
                    }
                    out.push((name, tag[value_start..i].to_string()));
                }
            } else {
                out.push((name, String::new()));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// A length in CSS pixels; a percentage stays as its number and is flagged.
///
/// Unitless numbers are pixels. The absolute units follow CSS: 96 px per inch,
/// 2.54 cm per inch, 25.4 mm per inch, 12 pt per pica and 16 px per pica.
fn length(value: &str) -> Option<(f64, bool)> {
    let text = value.trim_start();
    let (number, used) = number_at(text)?;
    let unit = text[used..].trim().to_ascii_lowercase();
    let pixels = match unit.as_str() {
        "" | "px" => number,
        "pt" => number * 4.0 / 3.0,
        "pc" => number * 16.0,
        "mm" => number * 96.0 / 25.4,
        "cm" => number * 96.0 / 2.54,
        "in" => number * 96.0,
        "%" => return Some((number, true)),
        _ => return None,
    };
    Some((pixels, false))
}

/// A number as the frozen Python reads it: digits, then an optional fraction.
/// The returned count is the bytes it used.
fn number_at(value: &str) -> Option<(f64, usize)> {
    let text = value.trim_start();
    let bytes = text.as_bytes();
    let mut digits = 0;
    while digits < bytes.len() && bytes[digits].is_ascii_digit() {
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    let mut end = digits;
    if end < bytes.len() && bytes[end] == b'.' {
        let mut frac = end + 1;
        while frac < bytes.len() && bytes[frac].is_ascii_digit() {
            frac += 1;
        }
        if frac > end + 1 {
            end = frac;
        }
    }
    text[..end].parse().ok().map(|value| (value, end))
}

/// Python's round: halves go to the even side.
pub fn round_half_even(value: f64) -> f64 {
    let floor = value.floor();
    if (value - floor - 0.5).abs() < 1e-9 {
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        value.round()
    }
}

/// The viewport for a drawing, deriving the missing side from the drawing's
/// own size. HTML pages carry no size, so they fall back to the flags.
pub fn resolve_size(
    svg: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<(u32, u32)> {
    let Some(text) = svg else {
        return Ok((width.unwrap_or(800), height.unwrap_or(600)));
    };
    let size = svg_size(text)?;
    let view_w = size.view_width.filter(|value| *value > 0.0);
    let view_h = size.view_height.filter(|value| *value > 0.0);
    // A fixed length is in pixels; a percentage resolves against the viewBox
    // side, then the other explicit side, then the 800x600 fallback.
    let fixed_w = if size.width_percent { None } else { size.width };
    let fixed_h = if size.height_percent {
        None
    } else {
        size.height
    };
    let base_w = if size.width_percent {
        size.width
            .map(|value| value / 100.0 * view_w.or(fixed_h).unwrap_or(800.0))
    } else {
        fixed_w
    };
    let base_h = if size.height_percent {
        size.height
            .map(|value| value / 100.0 * view_h.or(fixed_w).unwrap_or(600.0))
    } else {
        fixed_h
    };
    let base_w = base_w.or(view_w);
    let base_h = base_h.or(view_h);
    let scaled = |value: f64| (round_half_even(value) as i64).max(8) as u32;
    match (width, height) {
        (Some(width), Some(height)) => Ok((width, height)),
        (Some(width), None) => match (base_w, base_h) {
            (Some(base_w), Some(base_h)) if base_w > 0.0 => {
                Ok((width, scaled(width as f64 * base_h / base_w)))
            }
            _ => Ok((width, 600)),
        },
        (None, Some(height)) => match (base_w, base_h) {
            (Some(base_w), Some(base_h)) if base_h > 0.0 => {
                Ok((scaled(height as f64 * base_w / base_h), height))
            }
            _ => Ok((800, height)),
        },
        (None, None) => match (base_w, base_h) {
            (Some(base_w), Some(base_h)) => Ok((scaled(base_w), scaled(base_h))),
            _ => Ok((800, 600)),
        },
    }
}

/// The page an SVG is drawn in.
pub fn wrap_svg(svg: &str, base: &str, width: u32, height: u32, background: [u8; 3]) -> String {
    let css = format!(
        "html,body{{margin:0;padding:0;width:{width}px;height:{height}px;overflow:hidden;background:{};}}svg{{display:block;width:100%;height:100%;}}",
        hex(background)
    );
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><base href=\"{base}\"><style>{css}</style></head><body>{svg}</body></html>"
    )
}

/// The preset keyframes and the rule that binds them to the target.
pub fn preset_css(preset: Preset, duration: f64) -> Option<String> {
    let (frames, timing) = match preset {
        Preset::None => return None,
        Preset::Rotate => (
            "@keyframes gif-rotate{from{transform:rotate(0deg)}to{transform:rotate(360deg)}}",
            "linear",
        ),
        Preset::Pulse => (
            "@keyframes gif-pulse{0%{transform:scale(1)}50%{transform:scale(1.08)}100%{transform:scale(1)}}",
            "ease-in-out",
        ),
        Preset::Fade => (
            "@keyframes gif-fade{0%{opacity:1}50%{opacity:0.25}100%{opacity:1}}",
            "ease-in-out",
        ),
    };
    Some(format!(
        "{frames} .gif-target{{animation-name:gif-{name};animation-duration:{duration}s;animation-timing-function:{timing};animation-iteration-count:infinite;animation-fill-mode:both;transform-box:fill-box;transform-origin:center;}}",
        name = preset.name()
    ))
}

/// The script that prepares the page: preset, fonts, then pause every clock.
pub fn js_setup(
    html_page: bool,
    preset: Preset,
    selector: Option<&str>,
    duration: f64,
    background: [u8; 3],
) -> String {
    let mut script = String::from("(async () => {");
    if html_page {
        let css = format!(
            "html,body{{margin:0;padding:0;overflow:hidden;background:{};}}",
            hex(background)
        );
        script.push_str(&format!(
            "const base=document.createElement('style');base.textContent={};document.head.appendChild(base);",
            js_string(&css)
        ));
    }
    if let Some(css) = preset_css(preset, duration) {
        let target = selector.unwrap_or("svg");
        script.push_str(&format!(
            "const style=document.createElement('style');style.textContent={};document.head.appendChild(style);",
            js_string(&css)
        ));
        script.push_str(&format!(
            "const el=document.querySelector({});if(!el){{throw new Error({});}}el.classList.add('gif-target');",
            js_string(target),
            js_string(&format!("selector not found: {target}"))
        ));
    }
    script.push_str(
        "if (document.fonts && document.fonts.ready) { await document.fonts.ready; }\
         await new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done)));\
         document.getAnimations().forEach((a) => a.pause());\
         const svg = document.querySelector('svg');\
         if (svg && svg.pauseAnimations) { svg.pauseAnimations(); }\
         return true; })()",
    );
    script
}

/// The script that puts every clock at one time, in seconds.
pub fn js_seek(seconds: f64) -> String {
    format!(
        "(() => {{ const t = {seconds:?}; document.getAnimations().forEach((a) => {{ try {{ a.currentTime = t * 1000; }} catch (e) {{}} }}); const svg = document.querySelector('svg'); if (svg && svg.setCurrentTime) {{ svg.setCurrentTime(t); }} return true; }})()"
    )
}

/// A JSON string literal, which is also a JavaScript string literal.
fn js_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

/// A file path as a URL a browser can open.
pub fn file_url(path: &std::path::Path) -> Result<String> {
    let path = path
        .canonicalize()
        .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
    Ok(file_url_text(&path.to_string_lossy()))
}

/// Percent-encode the parts of a path a URL cannot carry.
fn file_url_text(path: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut text = path.replace('\\', "/");
    // Rust canonicalises to an extended-length path on Windows; a URL wants
    // the plain drive path or the UNC share path.
    if let Some(rest) = text.strip_prefix("//?/UNC/") {
        text = format!("//{rest}");
    } else if let Some(rest) = text.strip_prefix("//?/") {
        text = rest.to_string();
    }
    let text = text.as_str();
    // A UNC share becomes the host part of the URL; a drive path keeps the
    // empty-host form.
    let (prefix, body) = if text.starts_with("//") {
        ("file:", text)
    } else {
        ("file:///", text.strip_prefix('/').unwrap_or(text))
    };
    let mut url = String::with_capacity(body.len() + prefix.len());
    url.push_str(prefix);
    for byte in body.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                url.push(byte as char);
            }
            _ => {
                url.push('%');
                url.push(HEX[(byte >> 4) as usize] as char);
                url.push(HEX[(byte & 0x0f) as usize] as char);
            }
        }
    }
    url
}

fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAWING: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80" viewBox="0 0 240 160"></svg>"#;

    #[test]
    fn the_root_size_is_read_from_attributes() {
        let size = svg_size(DRAWING).unwrap();
        assert_eq!(size.width, Some(120.0));
        assert_eq!(size.height, Some(80.0));
        assert_eq!(size.view_width, Some(240.0));
        assert_eq!(size.view_height, Some(160.0));
    }

    #[test]
    fn single_quotes_and_comma_separated_viewboxes_work() {
        let text = "<svg width='40px' height='20' viewBox='0,0,400,200'></svg>";
        let size = svg_size(text).unwrap();
        assert_eq!(size.width, Some(40.0));
        assert_eq!(size.view_width, Some(400.0));
        assert_eq!(size.view_height, Some(200.0));
    }

    #[test]
    fn a_document_without_svg_is_rejected() {
        assert!(svg_size("<html><body></body></html>").is_err());
    }

    #[test]
    fn a_missing_attribute_is_not_a_size() {
        let size = svg_size("<svg viewBox='0 0 10 10'></svg>").unwrap();
        assert_eq!(size.width, None);
        assert_eq!(size.view_width, Some(10.0));
    }

    #[test]
    fn lengths_carry_units_and_percentages() {
        let size = svg_size("<svg width='10cm' height='1in'></svg>").unwrap();
        assert!((size.width.unwrap() - 377.95).abs() < 0.01);
        assert_eq!(size.height, Some(96.0));
        let size = svg_size("<svg width='100%' height='50%' viewBox='0 0 400 200'></svg>").unwrap();
        assert_eq!(size.width, Some(100.0));
        assert!(size.width_percent);
        assert!(size.height_percent);
    }

    #[test]
    fn a_percentage_resolves_against_the_viewbox() {
        let svg = Some("<svg width='100%' height='100%' viewBox='0 0 400 200'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (400, 200));
    }

    #[test]
    fn a_percentage_without_a_viewbox_uses_the_other_side() {
        let svg = Some("<svg width='50%' height='100'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (50, 100));
        let svg = Some("<svg width='200' height='25%'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (200, 50));
        let svg = Some("<svg width='50%' height='50%'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (400, 300));
    }

    #[test]
    fn centimetres_become_pixels() {
        let svg = Some("<svg width='10cm' height='10cm'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (378, 378));
    }

    #[test]
    fn sizes_are_derived_from_the_drawing() {
        let svg = Some(DRAWING);
        assert_eq!(resolve_size(svg, None, None).unwrap(), (120, 80));
        assert_eq!(resolve_size(svg, Some(60), None).unwrap(), (60, 40));
        assert_eq!(resolve_size(svg, None, Some(40)).unwrap(), (60, 40));
        assert_eq!(resolve_size(svg, Some(100), Some(50)).unwrap(), (100, 50));
        assert_eq!(resolve_size(None, None, None).unwrap(), (800, 600));
        assert_eq!(resolve_size(None, Some(320), None).unwrap(), (320, 600));
    }

    #[test]
    fn a_viewbox_stands_in_for_a_missing_size() {
        let svg = Some("<svg viewBox='0 0 300 150'></svg>");
        assert_eq!(resolve_size(svg, None, None).unwrap(), (300, 150));
    }

    #[test]
    fn tiny_derived_sides_are_raised_to_eight() {
        let svg = Some("<svg width='100' height='1'></svg>");
        assert_eq!(resolve_size(svg, Some(100), None).unwrap(), (100, 8));
    }

    #[test]
    fn presets_carry_their_own_timing() {
        assert!(preset_css(Preset::None, 2.0).is_none());
        let rotate = preset_css(Preset::Rotate, 2.0).unwrap();
        assert!(rotate.contains("gif-rotate"));
        assert!(rotate.contains("linear"));
        assert!(rotate.contains("animation-duration:2s"));
        let pulse = preset_css(Preset::Pulse, 3.0).unwrap();
        assert!(pulse.contains("scale(1.08)"));
        assert!(pulse.contains("ease-in-out"));
        let fade = preset_css(Preset::Fade, 1.5).unwrap();
        assert!(fade.contains("opacity:0.25"));
    }

    #[test]
    fn the_setup_script_names_the_selector_and_pauses() {
        let script = js_setup(true, Preset::Rotate, Some("#logo"), 2.0, [1, 2, 3]);
        assert!(script.contains("\"#logo\""));
        assert!(script.contains("selector not found: #logo"));
        assert!(script.contains("pauseAnimations"));
        assert!(script.contains("#010203"));
        assert!(script.starts_with("(async () => {"));
    }

    #[test]
    fn the_seek_script_sets_both_clock_kinds() {
        let script = js_seek(0.5);
        assert!(script.contains("currentTime = t * 1000"));
        assert!(script.contains("setCurrentTime(t)"));
        assert!(script.contains("const t = 0.5;"));
    }

    #[test]
    fn a_wrapped_svg_page_carries_the_base_url() {
        let page = wrap_svg(DRAWING, "file:///C:/art/", 120, 80, [255, 255, 255]);
        assert!(page.contains("<base href=\"file:///C:/art/\">"));
        assert!(page.contains("width:120px;height:80px"));
        assert!(page.contains("#ffffff"));
        assert!(page.contains(DRAWING));
    }

    #[test]
    fn paths_become_file_urls() {
        assert_eq!(file_url_text(r"C:\art\a b.svg"), "file:///C:/art/a%20b.svg");
        assert_eq!(file_url_text("/home/x/a.svg"), "file:///home/x/a.svg");
        assert_eq!(
            file_url_text(r"C:\art\100%.svg"),
            "file:///C:/art/100%25.svg"
        );
        assert_eq!(file_url_text(r"\\?\C:\art\a.svg"), "file:///C:/art/a.svg");
        assert_eq!(
            file_url_text(r"\\?\UNC\server\share\a.svg"),
            "file://server/share/a.svg"
        );
    }

    #[test]
    fn halves_round_to_even_like_python() {
        assert_eq!(round_half_even(0.5), 0.0);
        assert_eq!(round_half_even(1.5), 2.0);
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(2.6), 3.0);
    }
}
