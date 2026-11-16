//! SVG export of a prepared tween timeline.
//!
//! The plan is sampled at the presentation times the renderer would use. The
//! samples become CSS keyframes inside the drawing, so playback needs no
//! browser and no JavaScript. Layers, springs, arcs and rate are already baked
//! into the values. The last sample holds until the end of the iteration, and
//! a restart is the loop seam, which is what the last frame's delay does in the
//! GIF and APNG writers (part 12).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use anikit_core::{Error, OutputSummary, Result, temp_path};
use anikit_time::{FramePlan, LoopMode};
use anikit_tween::{Plan, Transform, Value};

use crate::import::parse_transform_list;

/// One property and the element id it belongs to, in plan order.
pub(crate) struct TargetRef<'a> {
    pub element: &'a str,
    pub slot: u16,
}

/// Write the exporter's view of the timeline into one SVG file.
pub(crate) fn write_svg(
    svg: &str,
    out: &Path,
    plan: &Plan,
    frames: &FramePlan,
    targets: &[TargetRef<'_>],
    width: u32,
    height: u32,
) -> Result<OutputSummary> {
    if plan.len() != targets.len() {
        return Err(Error::invalid(
            "the timeline plan and the export targets disagree",
        ));
    }
    let total_ms = frames.total_ms();
    if total_ms == 0 || frames.is_empty() {
        return Err(Error::invalid(
            "a timeline with no duration cannot be exported",
        ));
    }
    let close = svg
        .rfind("</svg>")
        .ok_or_else(|| Error::invalid("the drawing has no closing </svg> tag"))?;

    let authored = authored_transforms(svg, targets)?;
    let style = stylesheet(plan, frames, targets, total_ms, &authored)?;
    let mut exported = String::with_capacity(svg.len() + style.len() + 32);
    exported.push_str(&svg[..close]);
    exported.push_str("<style id=\"anikit\">\n");
    exported.push_str(&xml_text(&style));
    exported.push_str("</style>\n");
    exported.push_str(&svg[close..]);

    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let temp = temp_path(out);
    write_and_verify(&temp, out, exported.as_bytes())?;
    Ok(OutputSummary {
        frames: frames.len() as u32,
        total_ms,
        width,
        height,
    })
}

/// Build the style block: one rule per element, one keyframes rule per property.
fn stylesheet(
    plan: &Plan,
    frames: &FramePlan,
    targets: &[TargetRef<'_>],
    total_ms: u32,
    authored: &HashMap<String, Transform>,
) -> Result<String> {
    // Element order follows the first target, so the output is deterministic.
    let mut elements: Vec<(&str, Vec<usize>)> = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        match elements.iter_mut().find(|(id, _)| *id == target.element) {
            Some((_, list)) => list.push(index),
            None => elements.push((target.element, vec![index])),
        }
    }

    let repeat = if plan.timing().loop_mode == LoopMode::Once {
        "forwards"
    } else {
        "infinite"
    };

    let mut style = String::with_capacity(targets.len() * 256);
    for (element, list) in &elements {
        let has_transform = list.iter().any(|index| targets[*index].slot == 0);
        style.push_str(&selector(element));
        style.push_str(" {");
        if has_transform {
            style.push_str("transform-box:view-box;transform-origin:0 0;");
        }
        style.push_str("animation:");
        for (position, index) in list.iter().enumerate() {
            if position > 0 {
                style.push(',');
            }
            let _ = write!(style, "anikit-{index} {total_ms}ms linear {repeat}");
        }
        style.push_str("}\n");

        for index in list {
            let base = authored
                .get(*element)
                .copied()
                .unwrap_or(Transform::IDENTITY);
            let _ = write!(style, "@keyframes anikit-{index}{{");
            for frame in 0..frames.len() {
                let pts = frames
                    .presentation_us(frame)
                    .ok_or_else(|| Error::invalid("frame is outside the plan"))?;
                let value = plan
                    .property_value(*index, pts)
                    .ok_or_else(|| Error::invalid("property is outside the plan"))?;
                style.push_str(&percent(pts, total_ms));
                style.push_str(&declaration(
                    targets[*index].slot,
                    compose(targets[*index].slot, value, &base),
                )?);
            }
            // The last sample needs an explicit 100 percent key, or the
            // animation interpolates back to the authored value over the last
            // interval and the loop seam reverses.
            let last = frames
                .presentation_us(frames.len() - 1)
                .ok_or_else(|| Error::invalid("frame is outside the plan"))?;
            let value = plan
                .property_value(*index, last)
                .ok_or_else(|| Error::invalid("property is outside the plan"))?;
            style.push_str("100%");
            style.push_str(&declaration(
                targets[*index].slot,
                compose(targets[*index].slot, value, &base),
            )?);
            style.push_str("}\n");
        }
    }
    Ok(style)
}

/// Compose the timeline matrix with the element's authored transform.
///
/// The capture writes the timeline on a wrapper around the element, so the
/// authored transform applies first: the export uses timeline * authored.
fn compose(slot: u16, value: Value, authored: &Transform) -> Value {
    match value {
        Value::Transform(matrix) if slot == 0 => Value::Transform(matrix.mul(authored)),
        other => other,
    }
}

/// The authored transform of every target element.
///
/// This is not an XML parser: it walks start tags, skips comments, CDATA and
/// declarations, and reads one attribute value at a time. An id that is absent
/// from the drawing fails before a file is written.
fn authored_transforms(svg: &str, targets: &[TargetRef<'_>]) -> Result<HashMap<String, Transform>> {
    let mut wanted: Vec<&str> = Vec::new();
    for target in targets {
        if !wanted.contains(&target.element) {
            wanted.push(target.element);
        }
    }
    let mut found = HashMap::with_capacity(wanted.len());
    let mut rest = svg;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        if rest.starts_with("<!--") {
            let Some(end) = rest.find("-->") else { break };
            rest = &rest[end + 3..];
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let Some(end) = rest.find("]]>") else { break };
            rest = &rest[end + 3..];
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let Some(end) = rest.find('>') else { break };
            rest = &rest[end + 1..];
            continue;
        }
        let Some(end) = tag_end(rest) else { break };
        let tag = &rest[1..end];
        rest = &rest[end + 1..];
        let Some(id) = attribute(tag, "id") else {
            continue;
        };
        if !wanted.contains(&id) || found.contains_key(id) {
            continue;
        }
        let transform = match attribute(tag, "transform") {
            Some(text) => parse_transform_list(text).map_err(|error| {
                Error::invalid(format!("the transform on #{id} cannot be read: {error}"))
            })?,
            None => Transform::IDENTITY,
        };
        found.insert(id.to_string(), transform);
    }
    for id in wanted {
        if !found.contains_key(id) {
            return Err(Error::invalid(format!(
                "the drawing has no element with id \"{id}\""
            )));
        }
    }
    Ok(found)
}

/// The end of a start tag, ignoring `>` inside a quoted value.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (index, ch) in text.char_indices() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) => {}
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '>' => return Some(index),
                _ => {}
            },
        }
    }
    None
}

/// One attribute value from a start tag, without its quotes.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while pos < bytes.len()
            && (bytes[pos].is_ascii_alphanumeric()
                || matches!(bytes[pos], b'-' | b'_' | b':' | b'.'))
        {
            pos += 1;
        }
        if start == pos {
            pos += 1;
            continue;
        }
        let key = &tag[start..pos];
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() || bytes[pos] != b'=' {
            continue;
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            return None;
        }
        let quote = bytes[pos];
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        pos += 1;
        let value_start = pos;
        while pos < bytes.len() && bytes[pos] != quote {
            pos += 1;
        }
        if pos >= bytes.len() {
            return None;
        }
        let value = &tag[value_start..pos];
        pos += 1;
        if key == name {
            return Some(value);
        }
    }
    None
}

fn selector(element: &str) -> String {
    let mut out = String::with_capacity(element.len() + 8);
    out.push_str("[id=\"");
    for ch in element.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push_str("\"]");
    out
}

fn declaration(slot: u16, value: Value) -> Result<String> {
    let mut out = String::with_capacity(64);
    match (slot, value) {
        (0, Value::Transform(t)) => {
            let _ = write!(
                out,
                "{{transform:matrix({:.6},{:.6},{:.6},{:.6},{:.6},{:.6})}}",
                t.m11, t.m12, t.m21, t.m22, t.tx, t.ty
            );
        }
        (1, Value::Opacity(opacity)) => {
            let _ = write!(out, "{{opacity:{opacity:.4}}}");
        }
        (2, Value::Color(colour)) | (3, Value::Color(colour)) => {
            let [r, g, b, _] = colour.to_srgb8();
            let property = if slot == 2 { "fill" } else { "stroke" };
            let _ = write!(out, "{{{property}:#{r:02x}{g:02x}{b:02x}}}");
        }
        (4, Value::Number(width)) => {
            let _ = write!(out, "{{stroke-width:{width:.3}px}}");
        }
        (slot, value) => {
            return Err(Error::invalid(format!(
                "property slot {slot} cannot hold a {} value",
                value.kind_name()
            )));
        }
    }
    Ok(out)
}

/// Escape the style block for XML text: an id may contain a character that is
/// legal in CSS but not as raw XML text.
fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

fn percent(t_us: u64, total_ms: u32) -> String {
    let value = t_us as f64 / (total_ms as f64 * 1000.0) * 100.0;
    let mut text = format!("{value:.4}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text.push('%');
    text
}

/// Write the temp file, read it back, then move it into place.
fn write_and_verify(temp: &Path, out: &Path, bytes: &[u8]) -> Result<()> {
    let result = (|| -> Result<()> {
        std::fs::write(temp, bytes)?;
        let read = std::fs::read(temp)?;
        if read != bytes {
            return Err(Error::verify("the exported SVG read back differently"));
        }
        std::fs::rename(temp, out)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use anikit_tween::{Ease, Key, PropId, Property, Smooth, Timeline, Timing, Track, Transform};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/export")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plan(loop_mode: LoopMode) -> Plan {
        let start = Transform::translate(0.0, 0.0);
        let end = Transform::translate(60.0, 0.0);
        let track = Track::new(vec![
            Key::new(0, Value::Transform(start)),
            Key::new(1_000_000, Value::Transform(end)).ease(Ease::Linear),
        ])
        .smooth(Smooth::Eased);
        let timeline = Timeline::new(1_000_000, Timing::new(loop_mode, 1.0))
            .property(Property::new(PropId::new(0, 0), track));
        timeline.prepare().unwrap()
    }

    const DRAWING: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\"><rect id=\"box\" x=\"0\" y=\"0\" width=\"10\" height=\"10\"/></svg>";

    #[test]
    fn a_transform_becomes_one_keyframes_rule() {
        let dir = scratch("transform");
        let out = dir.join("out.svg");
        let summary = write_svg(
            DRAWING,
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(4, 4.0).unwrap(),
            &[TargetRef {
                element: "box",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap();
        assert_eq!(summary.frames, 4);
        assert_eq!(summary.total_ms, 1000);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("[id=\"box\"] {"));
        assert!(text.contains("transform-origin:0 0"));
        assert!(text.contains(
            "@keyframes anikit-0{0%{transform:matrix(1.000000,0.000000,0.000000,1.000000,0.000000,0.000000)}"
        ));
        assert!(text.contains(
            "75%{transform:matrix(1.000000,0.000000,0.000000,1.000000,45.000000,0.000000)}"
        ));
        assert!(text.contains(
            "75%{transform:matrix(1.000000,0.000000,0.000000,1.000000,45.000000,0.000000)}100%{transform:matrix(1.000000,0.000000,0.000000,1.000000,45.000000,0.000000)}"
        ));
        assert!(text.contains("animation:anikit-0 1000ms linear infinite"));
        assert!(!dir.join("out.tmp.svg").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_authored_transform_is_composed_under_the_timeline() {
        let dir = scratch("authored");
        let out = dir.join("out.svg");
        let drawing = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\"><rect id=\"box\" transform=\"translate(30 0)\" x=\"0\" y=\"0\" width=\"10\" height=\"10\"/></svg>";
        write_svg(
            drawing,
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(4, 4.0).unwrap(),
            &[TargetRef {
                element: "box",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains(
                "0%{transform:matrix(1.000000,0.000000,0.000000,1.000000,30.000000,0.000000)}"
            ),
            "the authored transform is the start"
        );
        assert!(
            text.contains(
                "75%{transform:matrix(1.000000,0.000000,0.000000,1.000000,75.000000,0.000000)}"
            ),
            "the timeline is composed left of the authored transform"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_target_is_refused_before_writing() {
        let dir = scratch("missing-target");
        let out = dir.join("out.svg");
        let err = write_svg(
            DRAWING,
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(2, 2.0).unwrap(),
            &[TargetRef {
                element: "not-there",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("no element with id \"not-there\""),
            "{err}"
        );
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_id_inside_a_comment_is_ignored() {
        let dir = scratch("comment");
        let out = dir.join("out.svg");
        let drawing = "<!-- <rect id=\"box\" transform=\"translate(99 0)\"/> --><svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\"><rect id=\"box\" transform=\"translate(5 0)\" x=\"0\" y=\"0\" width=\"10\" height=\"10\"/></svg>";
        write_svg(
            drawing,
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(2, 2.0).unwrap(),
            &[TargetRef {
                element: "box",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains(
                "0%{transform:matrix(1.000000,0.000000,0.000000,1.000000,5.000000,0.000000)}"
            ),
            "the element's transform, not the commented one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_quoted_id_is_found() {
        let dir = scratch("quotes");
        let out = dir.join("out.svg");
        let drawing = "<svg xmlns='http://www.w3.org/2000/svg' width='120' height='80'><rect id='box' transform='translate(7 0)' x='0' y='0' width='10' height='10'/></svg>";
        write_svg(
            drawing,
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(2, 2.0).unwrap(),
            &[TargetRef {
                element: "box",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(
            text.contains(
                "0%{transform:matrix(1.000000,0.000000,0.000000,1.000000,7.000000,0.000000)}"
            ),
            "single quotes carry the transform too"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_properties_on_one_element_share_one_rule() {
        let dir = scratch("two");
        let start = Transform::translate(0.0, 0.0);
        let end = Transform::translate(10.0, 0.0);
        let move_track = Track::new(vec![
            Key::new(0, Value::Transform(start)),
            Key::new(1_000_000, Value::Transform(end)),
        ]);
        let fade = Track::new(vec![
            Key::new(0, Value::Opacity(1.0)),
            Key::new(1_000_000, Value::Opacity(0.0)),
        ]);
        let timeline = Timeline::new(1_000_000, Timing::new(LoopMode::Once, 1.0)).properties(vec![
            Property::new(PropId::new(0, 0), move_track),
            Property::new(PropId::new(0, 1), fade),
        ]);
        let plan = timeline.prepare().unwrap();
        let out = dir.join("out.svg");
        write_svg(
            DRAWING,
            &out,
            &plan,
            &FramePlan::uniform(2, 2.0).unwrap(),
            &[
                TargetRef {
                    element: "box",
                    slot: 0,
                },
                TargetRef {
                    element: "box",
                    slot: 1,
                },
            ],
            120,
            80,
        )
        .unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert_eq!(text.matches("[id=\"box\"]").count(), 1);
        assert!(text.contains("anikit-0 1000ms linear forwards,anikit-1 1000ms linear forwards"));
        assert!(text.contains("{opacity:0.5000}"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_drawing_without_a_closing_tag_is_refused() {
        let dir = scratch("bad");
        let out = dir.join("bad.svg");
        let err = write_svg(
            "<svg></g>",
            &out,
            &plan(LoopMode::Cycle),
            &FramePlan::uniform(2, 2.0).unwrap(),
            &[TargetRef {
                element: "box",
                slot: 0,
            }],
            120,
            80,
        )
        .unwrap_err();
        assert!(err.to_string().contains("</svg>"));
        assert!(!out.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
