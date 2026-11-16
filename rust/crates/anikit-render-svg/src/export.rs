//! SVG export of a prepared tween timeline.
//!
//! The plan is sampled at the presentation times the renderer would use. The
//! samples become CSS keyframes inside the drawing, so playback needs no
//! browser and no JavaScript. Layers, springs, arcs and rate are already baked
//! into the values. The last sample holds until the end of the iteration, and
//! a restart is the loop seam, which is what the last frame's delay does in the
//! GIF and APNG writers (part 12).

use std::fmt::Write as _;
use std::path::Path;

use anikit_core::{Error, OutputSummary, Result, temp_path};
use anikit_time::{FramePlan, LoopMode};
use anikit_tween::{Plan, Value};

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

    let style = stylesheet(plan, frames, targets, total_ms)?;
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
            let _ = write!(style, "@keyframes anikit-{index}{{");
            for frame in 0..frames.len() {
                let pts = frames
                    .presentation_us(frame)
                    .ok_or_else(|| Error::invalid("frame is outside the plan"))?;
                let value = plan
                    .property_value(*index, pts)
                    .ok_or_else(|| Error::invalid("property is outside the plan"))?;
                style.push_str(&percent(pts, total_ms));
                style.push_str(&declaration(targets[*index].slot, value)?);
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
            style.push_str(&declaration(targets[*index].slot, value)?);
            style.push_str("}\n");
        }
    }
    Ok(style)
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
