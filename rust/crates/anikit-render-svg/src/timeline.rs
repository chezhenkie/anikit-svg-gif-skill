//! The tween route: a JSON timeline over one SVG drawing.
//!
//! Part 11 fixes the grammar. Parsing and prepare happen before a browser
//! starts; the page then receives one state write per frame and a screenshot.
//! Sampling is the tween crate's allocation-free path: one prepared plan and
//! one reused value buffer for the whole run.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anikit_core::{Dither, Error, Frame, FrameSink, Result, StreamOptions, stream_gif};
use anikit_time::FramePlan;
use anikit_tween::{
    Ballistic, Binding, Color, Decomposed, Ease, Key, Layer, LayerId, LoopMode, Motion, Op, Plan,
    PropId, Property, Smooth, Spring, StepAt, Timeline, Timing, Track, Transform, Value, Window,
};
use serde_json::Value as Json;

use crate::page::{self, Preset};
use crate::{Page, Rendered, discover, write_page};

/// How many frames feed the palette, as on every other route.
const SAMPLES: usize = 12;

/// Everything the tween route needs that is not the timeline or output path.
#[derive(Clone, Debug)]
pub struct TweenOptions {
    pub colors: usize,
    pub dither: Dither,
    pub loop_count: u16,
    pub browser: Option<PathBuf>,
}

/// Render one canonical timeline into a GIF.
pub fn write_tween(timeline: &Path, out: &Path, options: &TweenOptions) -> Result<Rendered> {
    let name = timeline
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| timeline.display().to_string());
    let text = std::fs::read_to_string(timeline)
        .map_err(|error| Error::io(format!("cannot read {}: {error}", timeline.display())))?;
    let base = timeline
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let scene = Scene::parse(&text, base, &name)?;

    let count = frame_count(scene.duration_us, scene.fps)?;
    let plan = FramePlan::uniform(count, scene.fps as f64)?;
    let svg = std::fs::read_to_string(&scene.drawing).map_err(|error| {
        Error::io(format!(
            "cannot read drawing {}: {error}",
            scene.drawing.display()
        ))
    })?;

    let exe = discover(options.browser.as_deref())?;
    let temp = write_page(
        &scene.drawing,
        &svg,
        scene.width,
        scene.height,
        scene.background,
    )?;
    let target = page::file_url(&temp.path)?;
    let mut page = Page::open(
        &exe,
        scene.width,
        scene.height,
        1,
        &target,
        scene.background,
    )?;
    // Pause every clock the drawing carries, then wrap the target elements.
    let setup = page::js_setup(false, Preset::None, None, 0.0, scene.background);
    page.evaluate(&setup, true)?;
    page.evaluate(&setup_script(&scene.targets), true)?;

    let mut values = vec![Value::Number(0.0); scene.plan.len()];
    let picks = SAMPLES.min(plan.len());
    let mut samples = Vec::with_capacity(picks);
    for pick in 0..picks {
        let index = if plan.len() <= picks {
            pick
        } else {
            (pick as f64 * (plan.len() - 1) as f64 / (picks - 1) as f64).round() as usize
        };
        samples.push(capture_frame(&mut page, &scene, &plan, index, &mut values)?);
    }

    let mut stream = stream_gif(
        out,
        &samples,
        StreamOptions {
            loop_count: options.loop_count,
            colors: options.colors,
            dither: options.dither,
            total_ms: plan.total_ms(),
        },
    )?;

    let mut first: Option<Frame> = None;
    for index in 0..plan.len() {
        let frame = capture_frame(&mut page, &scene, &plan, index, &mut values)?;
        if first.is_none() {
            first = Some(frame.clone());
        }
        let pts = plan
            .presentation_us(index)
            .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
        stream.push(index as u32, pts, &frame)?;
    }
    drop(page);
    let summary = stream.finish()?;
    let first = first.ok_or_else(|| Error::invalid("no frames were captured"))?;
    Ok(Rendered { summary, first })
}

/// The frame count: the duration rounded up, never fewer than two.
fn frame_count(duration_us: u64, fps: u32) -> Result<usize> {
    let seconds = duration_us as f64 / 1e6;
    let count = (seconds * fps as f64).ceil();
    if !count.is_finite() {
        return Err(Error::invalid("the timeline is too long to render"));
    }
    Ok((count as usize).max(2))
}

fn capture_frame(
    page: &mut Page,
    scene: &Scene,
    plan: &FramePlan,
    index: usize,
    values: &mut [Value],
) -> Result<Frame> {
    let pts = plan
        .presentation_us(index)
        .ok_or_else(|| Error::invalid(format!("frame {index} is outside the plan")))?;
    scene.plan.sample_into(pts, values);
    page.evaluate(&frame_script(&scene.targets, values), false)?;
    page.shoot()
}

// ---------------------------------------------------------------------------
// Prepared scene
// ---------------------------------------------------------------------------

/// A parsed timeline, the drawing it addresses and the element state it writes.
#[derive(Debug)]
struct Scene {
    drawing: PathBuf,
    width: u32,
    height: u32,
    background: [u8; 3],
    fps: u32,
    duration_us: u64,
    plan: Plan,
    targets: Vec<Target>,
}

/// One property and where it lands in the page.
#[derive(Debug)]
struct Target {
    element: String,
    slot: u16,
}

/// A base track, before element indices are assigned.
struct BaseTrack {
    id: String,
    kind: Kind,
    element: String,
    file: PathBuf,
    track: Track<Value>,
}

/// What a layer binding needs to know about a base track.
struct TrackInfo {
    id: String,
    kind: Kind,
    prop: PropId,
}

/// The five properties this route can draw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Transform,
    Opacity,
    Fill,
    Stroke,
    StrokeWidth,
}

impl Kind {
    fn parse(name: &str) -> Option<Kind> {
        match name {
            "svg.transform" => Some(Kind::Transform),
            "svg.opacity" => Some(Kind::Opacity),
            "svg.fill" => Some(Kind::Fill),
            "svg.stroke" => Some(Kind::Stroke),
            "svg.stroke-width" => Some(Kind::StrokeWidth),
            _ => None,
        }
    }

    fn slot(self) -> u16 {
        match self {
            Kind::Transform => 0,
            Kind::Opacity => 1,
            Kind::Fill => 2,
            Kind::Stroke => 3,
            Kind::StrokeWidth => 4,
        }
    }

    fn value_fields(self) -> &'static [&'static str] {
        match self {
            Kind::Transform => &["translate", "rotate", "scale", "skew"],
            Kind::Opacity | Kind::Fill | Kind::Stroke | Kind::StrokeWidth => &["value"],
        }
    }
}

impl Scene {
    fn parse(text: &str, base: &Path, file: &str) -> Result<Scene> {
        let root_value: Json = serde_json::from_str(text)
            .map_err(|error| Error::invalid(format!("{file}: not valid JSON: {error}")))?;
        let root = object(&root_value, file)?;
        allowed(
            root,
            &["version", "canvas", "timing", "tracks", "layers"],
            file,
        )?;

        let version_path = format!("{file}.version");
        if integer(required(root, "version", file)?, &version_path)? != 1 {
            return Err(err(&version_path, "must be 1"));
        }

        let canvas_path = format!("{file}.canvas");
        let canvas = object(required(root, "canvas", file)?, &canvas_path)?;
        allowed(canvas, &["w", "h", "bg", "fps"], &canvas_path)?;
        let width = side(
            required(canvas, "w", &canvas_path)?,
            &format!("{canvas_path}.w"),
        )?;
        let height = side(
            required(canvas, "h", &canvas_path)?,
            &format!("{canvas_path}.h"),
        )?;
        let background = match canvas.get("bg") {
            Some(value) => colour(value, &format!("{canvas_path}.bg"))?,
            None => [0, 0, 0],
        };
        let fps_path = format!("{canvas_path}.fps");
        let fps = integer(required(canvas, "fps", &canvas_path)?, &fps_path)?;
        if !(1..=50).contains(&fps) {
            return Err(err(&fps_path, "must be between 1 and 50"));
        }
        let fps = fps as u32;

        let timing = match root.get("timing") {
            Some(value) => parse_timing(value, &format!("{file}.timing"))?,
            None => Timing::default(),
        };

        let tracks_path = format!("{file}.tracks");
        let track_values = array(required(root, "tracks", file)?, &tracks_path)?;
        if track_values.is_empty() {
            return Err(err(&tracks_path, "a scene needs at least one track"));
        }
        let mut base_tracks = Vec::with_capacity(track_values.len());
        let mut track_ids = HashSet::new();
        for (index, item) in track_values.iter().enumerate() {
            let track = parse_track(item, &format!("{tracks_path}[{index}]"))?;
            if !track_ids.insert(track.id.clone()) {
                return Err(err(
                    &format!("{tracks_path}[{index}].id"),
                    format!("duplicate track id \"{}\"", track.id),
                ));
            }
            base_tracks.push(track);
        }

        // One drawing per scene, because the route loads one page.
        let mut drawing: Option<PathBuf> = None;
        for track in &base_tracks {
            let path = base.join(&track.file);
            let canonical = std::fs::canonicalize(&path).map_err(|error| {
                Error::io(format!("cannot read drawing {}: {error}", path.display()))
            })?;
            if !canonical
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
            {
                return Err(Error::invalid(format!(
                    "the drawing must be an .svg file: {}",
                    path.display()
                )));
            }
            match &drawing {
                Some(first) if *first != canonical => {
                    return Err(Error::invalid(format!(
                        "{file}: src file {} differs from {}; one scene uses one drawing",
                        canonical.display(),
                        first.display()
                    )));
                }
                Some(_) => {}
                None => drawing = Some(canonical),
            }
        }
        let drawing = drawing.expect("a scene has at least one track");

        // Elements, properties and the page targets, in file order.
        let mut element_index: HashMap<String, u32> = HashMap::new();
        let mut taken: HashMap<(u32, u16), String> = HashMap::new();
        let mut properties = Vec::with_capacity(base_tracks.len());
        let mut targets = Vec::with_capacity(base_tracks.len());
        let mut infos = Vec::with_capacity(base_tracks.len());
        for track in &base_tracks {
            let index = if let Some(index) = element_index.get(&track.element).copied() {
                index
            } else {
                let index = element_index.len() as u32;
                element_index.insert(track.element.clone(), index);
                index
            };
            let slot = track.kind.slot();
            if let Some(first) = taken.insert((index, slot), track.id.clone()) {
                return Err(Error::invalid(format!(
                    "{file}: tracks \"{first}\" and \"{}\" animate the same property of #{}",
                    track.id, track.element
                )));
            }
            let prop = PropId::new(index, slot);
            properties.push(Property::new(prop, track.track.clone()));
            targets.push(Target {
                element: track.element.clone(),
                slot,
            });
            infos.push(TrackInfo {
                id: track.id.clone(),
                kind: track.kind,
                prop,
            });
        }

        let layers_path = format!("{file}.layers");
        let layers = match root.get("layers") {
            Some(value) => parse_layers(array(value, &layers_path)?, &layers_path, &infos)?,
            None => Vec::new(),
        };

        // The duration is the last key of any track, binding or weight.
        let mut duration_us = 0u64;
        for track in &base_tracks {
            for key in &track.track.keys {
                duration_us = duration_us.max(key.t_us);
            }
        }
        for layer in &layers {
            for key in &layer.weight.keys {
                duration_us = duration_us.max(key.t_us);
            }
            for binding in &layer.bindings {
                for key in &binding.track.keys {
                    duration_us = duration_us.max(key.t_us);
                }
                if let Some(weight) = &binding.weight {
                    for key in &weight.keys {
                        duration_us = duration_us.max(key.t_us);
                    }
                }
            }
        }

        let mut timeline = Timeline::new(duration_us, timing).properties(properties);
        for layer in layers {
            timeline = timeline.layer(layer);
        }
        let plan = timeline
            .prepare()
            .map_err(|error| Error::invalid(format!("{file}: {error}")))?;

        Ok(Scene {
            drawing,
            width,
            height,
            background,
            fps,
            duration_us,
            plan,
            targets,
        })
    }
}

fn parse_timing(value: &Json, path: &str) -> Result<Timing> {
    let map = object(value, path)?;
    allowed(map, &["loop", "rate"], path)?;
    let loop_mode = match map.get("loop") {
        None => LoopMode::Once,
        Some(value) => match text(value, &format!("{path}.loop"))? {
            "once" => LoopMode::Once,
            "cycle" => LoopMode::Cycle,
            "pingpong" => LoopMode::PingPong,
            other => {
                return Err(err(
                    &format!("{path}.loop"),
                    format!("loop must be once, cycle or pingpong, got {other}"),
                ));
            }
        },
    };
    let rate = match map.get("rate") {
        Some(value) => number(value, &format!("{path}.rate"))?,
        None => 1.0,
    };
    Ok(Timing::new(loop_mode, rate))
}

fn parse_track(item: &Json, path: &str) -> Result<BaseTrack> {
    let map = object(item, path)?;
    allowed(map, &["id", "kind", "src", "smooth", "keys"], path)?;
    let id_path = format!("{path}.id");
    let id = text(required(map, "id", path)?, &id_path)?.to_string();
    if id.is_empty() {
        return Err(err(&id_path, "must not be empty"));
    }
    let kind_path = format!("{path}.kind");
    let kind_name = text(required(map, "kind", path)?, &kind_path)?;
    let kind = Kind::parse(kind_name).ok_or_else(|| {
        err(
            &kind_path,
            format!(
                "unknown kind {kind_name}; expected svg.fill, svg.opacity, svg.stroke, svg.stroke-width or svg.transform"
            ),
        )
    })?;
    let src_path = format!("{path}.src");
    let src = text(required(map, "src", path)?, &src_path)?;
    let (file, element) = src
        .rsplit_once('#')
        .ok_or_else(|| err(&src_path, "src is file.svg#elementId"))?;
    if file.is_empty() || element.is_empty() {
        return Err(err(&src_path, "src is file.svg#elementId"));
    }
    let smooth = parse_smooth(map.get("smooth"), &format!("{path}.smooth"))?;
    let keys_path = format!("{path}.keys");
    let keys = array(required(map, "keys", path)?, &keys_path)?;
    if keys.is_empty() {
        return Err(err(&keys_path, "a track needs at least one key"));
    }
    let track = parse_keys(kind, keys, &keys_path)?.smooth(smooth);
    track.prepare_named(&format!("{path}: track \"{id}\""))?;
    Ok(BaseTrack {
        id,
        kind,
        element: element.to_string(),
        file: PathBuf::from(file),
        track,
    })
}

fn parse_smooth(value: Option<&Json>, path: &str) -> Result<Smooth> {
    match value {
        None => Ok(Smooth::Eased),
        Some(value) => match text(value, path)? {
            "eased" => Ok(Smooth::Eased),
            "auto" => Ok(Smooth::Auto),
            "monotone" => Ok(Smooth::Monotone),
            other => Err(err(
                path,
                format!("smooth must be auto, eased or monotone, got {other}"),
            )),
        },
    }
}

fn parse_keys(kind: Kind, items: &[Json], path: &str) -> Result<Track<Value>> {
    let mut components = Decomposed {
        tx: 0.0,
        ty: 0.0,
        rotation: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        skew: 0.0,
    };
    let mut keys = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{path}[{index}]");
        let map = object(item, &at)?;
        let mut names = vec!["t", "easing", "motion", "tangent"];
        names.extend_from_slice(kind.value_fields());
        allowed(map, &names, &at)?;
        let t_us = microseconds(required(map, "t", &at)?, &format!("{at}.t"))?;
        let motion = parse_segment_motion(map, &at)?;
        let tangent = match map.get("tangent") {
            Some(value) => Some(number(value, &format!("{at}.tangent"))?),
            None => None,
        };
        let value = parse_value(kind, map, &at, &mut components)?;
        let mut key = Key::new(t_us, value).motion(motion);
        if let Some(tangent) = tangent {
            key = key.tangent(tangent);
        }
        keys.push(key);
    }
    Ok(Track::new(keys))
}

fn parse_value(kind: Kind, map: &Object, path: &str, components: &mut Decomposed) -> Result<Value> {
    match kind {
        Kind::Transform => {
            if let Some(value) = map.get("translate") {
                let pair = pair(value, &format!("{path}.translate"))?;
                components.tx = pair[0];
                components.ty = pair[1];
            }
            if let Some(value) = map.get("rotate") {
                components.rotation = number(value, &format!("{path}.rotate"))?.to_radians();
            }
            if let Some(value) = map.get("scale") {
                let values = scale(value, &format!("{path}.scale"))?;
                components.scale_x = values[0];
                components.scale_y = values[1];
            }
            if let Some(value) = map.get("skew") {
                components.skew = number(value, &format!("{path}.skew"))?.to_radians();
            }
            Ok(Value::Transform(Transform::from_decomposed(components)))
        }
        Kind::Opacity => {
            let at = format!("{path}.value");
            let value = number(required(map, "value", path)?, &at)?;
            if !(0.0..=1.0).contains(&value) {
                return Err(err(&at, "opacity must be between 0 and 1"));
            }
            Ok(Value::Opacity(value as f32))
        }
        Kind::Fill | Kind::Stroke => {
            let at = format!("{path}.value");
            let [r, g, b] = colour(required(map, "value", path)?, &at)?;
            Ok(Value::Color(Color::srgb8(r, g, b, 255)))
        }
        Kind::StrokeWidth => {
            let at = format!("{path}.value");
            let value = number(required(map, "value", path)?, &at)?;
            if value < 0.0 {
                return Err(err(&at, "stroke width must be 0 or more"));
            }
            Ok(Value::Number(value))
        }
    }
}

fn parse_segment_motion(map: &Object, path: &str) -> Result<Motion> {
    match (map.get("easing"), map.get("motion")) {
        (Some(_), Some(_)) => Err(err(path, "use easing or motion, not both")),
        (Some(easing), None) => Ok(Motion::Ease(parse_easing(
            easing,
            &format!("{path}.easing"),
        )?)),
        (None, Some(motion)) => parse_motion(motion, &format!("{path}.motion")),
        (None, None) => Ok(Motion::Ease(Ease::Linear)),
    }
}

fn parse_easing(value: &Json, path: &str) -> Result<Ease> {
    match value {
        Json::String(name) => match name.as_str() {
            "linear" => Ok(Ease::Linear),
            "ease" => Ok(Ease::cubic(0.25, 0.1, 0.25, 1.0)),
            "ease-in" => Ok(Ease::cubic(0.42, 0.0, 1.0, 1.0)),
            "ease-out" => Ok(Ease::cubic(0.0, 0.0, 0.58, 1.0)),
            "ease-in-out" => Ok(Ease::cubic(0.42, 0.0, 0.58, 1.0)),
            "hold" => Ok(Ease::Hold),
            other => Err(err(
                path,
                format!(
                    "easing must be ease, ease-in, ease-in-out, ease-out, hold or linear; got {other}"
                ),
            )),
        },
        Json::Object(map) => {
            let mut names = Vec::new();
            if map.contains_key("bezier") {
                names.push("bezier");
            }
            if map.contains_key("steps") {
                names.push("steps");
            }
            if map.contains_key("spring") {
                names.push("spring");
            }
            if names.len() != 1 {
                return Err(err(
                    path,
                    "an easing object needs exactly one of: bezier, steps, spring",
                ));
            }
            if let Some(value) = map.get("bezier") {
                allowed(map, &["bezier"], path)?;
                let [x1, y1, x2, y2] = four(value, &format!("{path}.bezier"))?;
                Ok(Ease::cubic(x1, y1, x2, y2))
            } else if map.contains_key("steps") {
                allowed(map, &["steps", "at"], path)?;
                let at_path = format!("{path}.at");
                let count = integer(required(map, "steps", path)?, &format!("{path}.steps"))?;
                if count < 1 {
                    return Err(err(&format!("{path}.steps"), "must be 1 or more"));
                }
                let at = match map.get("at") {
                    None => StepAt::End,
                    Some(value) => match text(value, &at_path)? {
                        "start" => StepAt::Start,
                        "end" => StepAt::End,
                        "none" => StepAt::None,
                        "both" => StepAt::Both,
                        other => {
                            return Err(err(
                                &at_path,
                                format!("must be both, end, none or start; got {other}"),
                            ));
                        }
                    },
                };
                Ok(Ease::steps(count as u32, at))
            } else {
                allowed(map, &["spring"], path)?;
                let spring_path = format!("{path}.spring");
                let spring = object(map.get("spring").expect("spring was checked"), &spring_path)?;
                parse_spring(spring, &spring_path)
            }
        }
        _ => Err(err(path, "easing must be a string or an object")),
    }
}

fn parse_spring(map: &Object, path: &str) -> Result<Ease> {
    allowed(
        map,
        &[
            "stiffness",
            "damping",
            "mass",
            "v0",
            "settle",
            "restDelta",
            "restSpeed",
        ],
        path,
    )?;
    let stiffness = maybe_number(map, "stiffness", path, 400.0)?;
    let damping = maybe_number(map, "damping", path, 30.0)?;
    let mass = maybe_number(map, "mass", path, 1.0)?;
    let v0 = maybe_number(map, "v0", path, 0.0)?;
    let rest_delta = maybe_number(map, "restDelta", path, 0.5)?;
    let rest_speed = maybe_number(map, "restSpeed", path, 2.0)?;
    let settle_path = format!("{path}.settle");
    let settle = match map.get("settle") {
        Some(value) => {
            let ms = integer(value, &settle_path)?;
            if ms <= 0 {
                return Err(err(&settle_path, "must be greater than 0"));
            }
            microseconds(value, &settle_path)?;
            ms as u64
        }
        None => 400,
    };
    if stiffness <= 0.0 {
        return Err(err(&format!("{path}.stiffness"), "must be greater than 0"));
    }
    if damping < 0.0 {
        return Err(err(&format!("{path}.damping"), "must be 0 or more"));
    }
    if mass <= 0.0 {
        return Err(err(&format!("{path}.mass"), "must be greater than 0"));
    }
    if rest_delta <= 0.0 {
        return Err(err(&format!("{path}.restDelta"), "must be greater than 0"));
    }
    if rest_speed <= 0.0 {
        return Err(err(&format!("{path}.restSpeed"), "must be greater than 0"));
    }
    Ok(Ease::Spring(
        Spring::new(stiffness, damping, mass, settle as u64 * 1000)
            .velocity(v0)
            .rest(rest_delta, rest_speed),
    ))
}

fn parse_motion(value: &Json, path: &str) -> Result<Motion> {
    let map = object(value, path)?;
    allowed(map, &["ballistic", "arc"], path)?;
    let ballistic = map.contains_key("ballistic");
    let arc = map.contains_key("arc");
    if ballistic == arc {
        return Err(err(path, "motion needs exactly one of: arc, ballistic"));
    }
    if ballistic {
        allowed(map, &["ballistic"], path)?;
        let at = format!("{path}.ballistic");
        let map = object(map.get("ballistic").expect("ballistic was checked"), &at)?;
        allowed(
            map,
            &[
                "plane",
                "gravity",
                "v0",
                "restitution",
                "friction",
                "minBounce",
                "maxSpeed",
                "contactHold",
                "squash",
                "stretch",
            ],
            &at,
        )?;
        let plane = number(required(map, "plane", &at)?, &format!("{at}.plane"))?;
        let mut motion = Ballistic::drop(plane);
        motion.gravity = maybe_number(map, "gravity", &at, motion.gravity)?;
        motion.v0 = maybe_number(map, "v0", &at, motion.v0)?;
        motion.restitution = maybe_number(map, "restitution", &at, motion.restitution)?;
        motion.friction = maybe_number(map, "friction", &at, motion.friction)?;
        motion.min_bounce = maybe_number(map, "minBounce", &at, motion.min_bounce)?;
        motion.max_speed = maybe_number(map, "maxSpeed", &at, motion.max_speed)?;
        motion.squash = maybe_number(map, "squash", &at, motion.squash)?;
        motion.stretch = maybe_number(map, "stretch", &at, motion.stretch)?;
        if let Some(value) = map.get("contactHold") {
            let hold_path = format!("{at}.contactHold");
            let ms = integer(value, &hold_path)?;
            if ms < 0 {
                return Err(err(&hold_path, "must be 0 or more"));
            }
            motion.contact_hold_us = ms as u64 * 1000;
        }
        Ok(Motion::Ballistic(motion))
    } else {
        allowed(map, &["arc"], path)?;
        let at = format!("{path}.arc");
        let map = object(map.get("arc").expect("arc was checked"), &at)?;
        allowed(map, &["bend", "easing"], &at)?;
        let bend = number(required(map, "bend", &at)?, &format!("{at}.bend"))?;
        let ease = match map.get("easing") {
            Some(value) => parse_easing(value, &format!("{at}.easing"))?,
            None => Ease::Linear,
        };
        Ok(Motion::Arc { bend, ease })
    }
}

fn parse_layers(items: &[Json], path: &str, tracks: &[TrackInfo]) -> Result<Vec<Layer>> {
    let mut layers = Vec::with_capacity(items.len());
    let mut ids = HashSet::new();
    let mut orders = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        let at = format!("{path}[{index}]");
        let map = object(item, &at)?;
        allowed(map, &["id", "order", "window", "weight", "bindings"], &at)?;
        let id_path = format!("{at}.id");
        let id = text(required(map, "id", &at)?, &id_path)?;
        if id.is_empty() {
            return Err(err(&id_path, "must not be empty"));
        }
        if !ids.insert(id.to_string()) {
            return Err(err(&id_path, format!("duplicate layer id \"{id}\"")));
        }
        let order_path = format!("{at}.order");
        let order = integer(required(map, "order", &at)?, &order_path)?;
        if order < 0 || order > u32::MAX as i64 {
            return Err(err(&order_path, "must be 0 or more"));
        }
        let order = order as u32;
        if !orders.insert(order) {
            return Err(err(&order_path, format!("order {order} is used twice")));
        }
        let weight = match map.get("weight") {
            Some(value) => {
                let weight_path = format!("{at}.weight");
                parse_scalar_track(array(value, &weight_path)?, &weight_path)?
            }
            None => Track::constant(1.0),
        };
        let mut layer = Layer::new(LayerId(index as u32 + 1), order, weight);
        if let Some(value) = map.get("window") {
            let window_path = format!("{at}.window");
            let ends = array(value, &window_path)?;
            if ends.len() != 2 {
                return Err(err(&window_path, "must be [start, end] in milliseconds"));
            }
            let start = microseconds(&ends[0], &format!("{window_path}[0]"))?;
            let end = microseconds(&ends[1], &format!("{window_path}[1]"))?;
            if end <= start {
                return Err(err(&window_path, "the window must end after it starts"));
            }
            layer = layer.window(Window::new(start, end));
        }
        let bindings_path = format!("{at}.bindings");
        let bindings = array(required(map, "bindings", &at)?, &bindings_path)?;
        if bindings.is_empty() {
            return Err(err(&bindings_path, "a layer needs at least one binding"));
        }
        for (bindex, binding) in bindings.iter().enumerate() {
            layer = layer.binding(parse_binding(
                binding,
                &format!("{bindings_path}[{bindex}]"),
                tracks,
            )?);
        }
        layers.push(layer);
    }
    Ok(layers)
}

fn parse_binding(item: &Json, path: &str, tracks: &[TrackInfo]) -> Result<Binding> {
    let map = object(item, path)?;
    allowed(
        map,
        &[
            "target",
            "op",
            "mode",
            "overshoot",
            "allowColorAdd",
            "weight",
            "keys",
        ],
        path,
    )?;
    let target_path = format!("{path}.target");
    let target = text(required(map, "target", path)?, &target_path)?;
    let info = tracks
        .iter()
        .find(|info| info.id == target)
        .ok_or_else(|| err(&target_path, format!("no track \"{target}\"")))?;
    let op = match map.get("op") {
        None => Op::Replace,
        Some(value) => {
            let at = format!("{path}.op");
            match text(value, &at)? {
                "replace" => Op::Replace,
                "add" => Op::Add,
                "multiply" => Op::Multiply,
                "accumulate" => Op::Accumulate,
                other => {
                    return Err(err(
                        &at,
                        format!("op must be accumulate, add, multiply or replace, got {other}"),
                    ));
                }
            }
        }
    };
    let keys_path = format!("{path}.keys");
    let track = parse_keys(
        info.kind,
        array(required(map, "keys", path)?, &keys_path)?,
        &keys_path,
    )?;
    track.prepare_named(&format!("{path}: track \"{target}\""))?;
    let mut binding = Binding::new(info.prop, op, track);
    match map.get("mode") {
        None => {}
        Some(value) => {
            let at = format!("{path}.mode");
            match text(value, &at)? {
                "append" => {}
                "prepend" => binding = binding.prepend(),
                other => {
                    return Err(err(
                        &at,
                        format!("mode must be append or prepend, got {other}"),
                    ));
                }
            }
        }
    }
    if map
        .get("overshoot")
        .map(|value| boolean(value, &format!("{path}.overshoot")))
        .transpose()?
        .unwrap_or(false)
    {
        binding = binding.overshoot();
    }
    if map
        .get("allowColorAdd")
        .map(|value| boolean(value, &format!("{path}.allowColorAdd")))
        .transpose()?
        .unwrap_or(false)
    {
        binding = binding.allow_color_add();
    }
    if let Some(value) = map.get("weight") {
        let weight_path = format!("{path}.weight");
        binding = binding.weight(parse_scalar_track(
            array(value, &weight_path)?,
            &weight_path,
        )?);
    }
    Ok(binding)
}

fn parse_scalar_track(items: &[Json], path: &str) -> Result<Track<f64>> {
    if items.is_empty() {
        return Err(err(path, "a weight track needs at least one key"));
    }
    let mut keys = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let at = format!("{path}[{index}]");
        let map = object(item, &at)?;
        allowed(map, &["t", "value", "easing", "tangent"], &at)?;
        let t_us = microseconds(required(map, "t", &at)?, &format!("{at}.t"))?;
        let value = number(required(map, "value", &at)?, &format!("{at}.value"))?;
        let ease = match map.get("easing") {
            Some(value) => parse_easing(value, &format!("{at}.easing"))?,
            None => Ease::Linear,
        };
        let mut key = Key::new(t_us, value).ease(ease);
        if let Some(value) = map.get("tangent") {
            key = key.tangent(number(value, &format!("{at}.tangent"))?);
        }
        keys.push(key);
    }
    let track = Track::new(keys);
    track.prepare_named(path)?;
    Ok(track)
}

// ---------------------------------------------------------------------------
// Page state
// ---------------------------------------------------------------------------

/// The setup script: wrap every target in an empty group and keep the handles.
fn setup_script(targets: &[Target]) -> String {
    let mut ids: Vec<&str> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut states: Vec<(usize, u16)> = Vec::with_capacity(targets.len());
    for target in targets {
        let at = if let Some(at) = index.get(target.element.as_str()).copied() {
            at
        } else {
            let at = ids.len();
            ids.push(target.element.as_str());
            index.insert(target.element.as_str(), at);
            at
        };
        states.push((at, target.slot));
    }
    let ids = serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string());
    let states: Vec<String> = states
        .iter()
        .map(|(element, slot)| format!("{{e:{element},slot:{slot}}}"))
        .collect();
    let mut script = String::with_capacity(512 + ids.len() + states.len() * 20);
    script.push_str("(async () => {");
    let _ = write!(script, "const ids = {ids};");
    script.push_str(
        "const els = ids.map((id) => {\
           const el = document.getElementById(id);\
           if (!el) { throw new Error('element not found: #' + id); }\
           if (!el.ownerSVGElement) { throw new Error('src must name an element inside the drawing: #' + id); }\
           const g = document.createElementNS('http://www.w3.org/2000/svg', 'g');\
           el.parentNode.insertBefore(g, el);\
           g.appendChild(el);\
           return { el: el, g: g };\
         });",
    );
    let _ = write!(
        script,
        "const states = [{}].map((s) => ({{ el: els[s.e].el, g: els[s.e].g, slot: s.slot }}));",
        states.join(",")
    );
    script.push_str("window.__anikit = { states: states }; return true; })()");
    script
}

/// The per-frame script: one write per property, in plan order.
fn frame_script(targets: &[Target], values: &[Value]) -> String {
    let mut script = String::with_capacity(64 + values.len() * 80);
    script.push_str("(() => { const s = window.__anikit.states;");
    for (index, (target, value)) in targets.iter().zip(values).enumerate() {
        match (target.slot, value) {
            (0, Value::Transform(transform)) => {
                let _ = write!(
                    script,
                    "s[{index}].g.setAttribute('transform','matrix({:.6},{:.6},{:.6},{:.6},{:.6},{:.6})');",
                    transform.m11,
                    transform.m12,
                    transform.m21,
                    transform.m22,
                    transform.tx,
                    transform.ty
                );
            }
            (1, Value::Opacity(opacity)) => {
                let _ = write!(script, "s[{index}].el.style.opacity='{opacity:.4}';");
            }
            (2, Value::Color(colour)) | (3, Value::Color(colour)) => {
                let [r, g, b, _] = colour.to_srgb8();
                let property = if target.slot == 2 { "fill" } else { "stroke" };
                let _ = write!(
                    script,
                    "s[{index}].el.style.{property}='#{r:02x}{g:02x}{b:02x}';"
                );
            }
            (4, Value::Number(width)) => {
                let _ = write!(script, "s[{index}].el.style.strokeWidth='{width:.3}px';");
            }
            _ => {}
        }
    }
    script.push_str(" return true; })()");
    script
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

type Object = serde_json::Map<String, Json>;

fn err(path: &str, message: impl std::fmt::Display) -> Error {
    Error::invalid(format!("{path}: {message}"))
}

fn object<'a>(value: &'a Json, path: &str) -> Result<&'a Object> {
    value
        .as_object()
        .ok_or_else(|| err(path, "must be an object"))
}

fn array<'a>(value: &'a Json, path: &str) -> Result<&'a Vec<Json>> {
    value
        .as_array()
        .ok_or_else(|| err(path, "must be an array"))
}

fn text<'a>(value: &'a Json, path: &str) -> Result<&'a str> {
    value.as_str().ok_or_else(|| err(path, "must be a string"))
}

fn number(value: &Json, path: &str) -> Result<f64> {
    match value.as_f64() {
        Some(number) if number.is_finite() => Ok(number),
        Some(_) => Err(err(path, "must be finite")),
        None => Err(err(path, "must be a number")),
    }
}

fn integer(value: &Json, path: &str) -> Result<i64> {
    value
        .as_i64()
        .ok_or_else(|| err(path, "must be an integer"))
}

fn boolean(value: &Json, path: &str) -> Result<bool> {
    value
        .as_bool()
        .ok_or_else(|| err(path, "must be true or false"))
}

fn required<'a>(map: &'a Object, name: &str, path: &str) -> Result<&'a Json> {
    map.get(name)
        .ok_or_else(|| err(path, format!("missing field \"{name}\"")))
}

fn allowed(map: &Object, names: &[&str], path: &str) -> Result<()> {
    for key in map.keys() {
        if !names.contains(&key.as_str()) {
            return Err(err(path, format!("unknown field \"{key}\"")));
        }
    }
    Ok(())
}

fn maybe_number(map: &Object, name: &str, path: &str, default: f64) -> Result<f64> {
    match map.get(name) {
        Some(value) => number(value, &format!("{path}.{name}")),
        None => Ok(default),
    }
}

fn microseconds(value: &Json, path: &str) -> Result<u64> {
    let ms = integer(value, path)?;
    if ms < 0 {
        return Err(err(path, "time must be 0 or more"));
    }
    (ms as u64)
        .checked_mul(1000)
        .ok_or_else(|| err(path, "time is too large"))
}

fn side(value: &Json, path: &str) -> Result<u32> {
    let side = integer(value, path)?;
    if !(8..=8192).contains(&side) {
        return Err(err(path, "must be between 8 and 8192"));
    }
    Ok(side as u32)
}

fn colour(value: &Json, path: &str) -> Result<[u8; 3]> {
    let body = text(value, path)?;
    let body = body
        .strip_prefix('#')
        .ok_or_else(|| err(path, "a colour is #rgb or #rrggbb"))?;
    let hex: String = match body.len() {
        3 => body.chars().flat_map(|c| [c, c]).collect(),
        6 => body.to_string(),
        _ => return Err(err(path, "a colour is #rgb or #rrggbb")),
    };
    if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(err(path, "a colour is #rgb or #rrggbb"));
    }
    let mut rgb = [0u8; 3];
    for (index, channel) in rgb.iter_mut().enumerate() {
        *channel = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| err(path, "a colour is #rgb or #rrggbb"))?;
    }
    Ok(rgb)
}

fn pair(value: &Json, path: &str) -> Result<[f64; 2]> {
    let items = array(value, path)?;
    if items.len() != 2 {
        return Err(err(path, "must be an array of two numbers"));
    }
    Ok([
        number(&items[0], &format!("{path}[0]"))?,
        number(&items[1], &format!("{path}[1]"))?,
    ])
}

fn scale(value: &Json, path: &str) -> Result<[f64; 2]> {
    if value.is_number() {
        let scale = number(value, path)?;
        return Ok([scale, scale]);
    }
    pair(value, path)
}

fn four(value: &Json, path: &str) -> Result<[f64; 4]> {
    let items = array(value, path)?;
    if items.len() != 4 {
        return Err(err(path, "must be an array of four numbers"));
    }
    let mut values = [0.0; 4];
    for (index, value) in items.iter().enumerate() {
        values[index] = number(value, &format!("{path}[{index}]"))?;
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/tween-parse")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("art.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect id="box" x="10" y="10" width="20" height="20" fill="#c82020"/></svg>"##,
        )
        .unwrap();
        dir
    }

    const SCENE: &str = r##"{
        "version": 1,
        "canvas": {"w": 120, "h": 80, "bg": "#ffffff", "fps": 10},
        "timing": {"loop": "once", "rate": 1},
        "tracks": [
            {"id": "move", "kind": "svg.transform", "src": "art.svg#box",
             "keys": [
                {"t": 0, "translate": [0, 0], "rotate": 0, "easing": "ease-out"},
                {"t": 400, "translate": [20, 0]},
                {"t": 1000, "translate": [60, 0], "rotate": 90}
             ]},
            {"id": "fade", "kind": "svg.opacity", "src": "art.svg#box",
             "keys": [
                {"t": 0, "value": 1.0},
                {"t": 1000, "value": 0.4, "easing": "hold"}
             ]},
            {"id": "paint", "kind": "svg.fill", "src": "art.svg#box",
             "keys": [
                {"t": 0, "value": "#c82020"},
                {"t": 1000, "value": "#2040d0"}
             ]}
        ],
        "layers": [
            {"id": "flash", "order": 1, "window": [300, 700],
             "weight": [{"t": 0, "value": 1}, {"t": 1000, "value": 0}],
             "bindings": [
                {"target": "fade", "op": "add", "keys": [{"t": 0, "value": 0.2}]}
             ]}
        ]
    }"##;

    fn parsed(name: &str) -> (PathBuf, Scene) {
        let dir = scratch(name);
        let scene = Scene::parse(SCENE, &dir, "scene.json").unwrap();
        (dir, scene)
    }

    #[test]
    fn a_scene_parses_and_prepares() {
        let (dir, scene) = parsed("full");
        assert_eq!(scene.duration_us, 1_000_000);
        assert_eq!(scene.plan.len(), 3);
        assert_eq!(scene.targets.len(), 3);
        assert_eq!(frame_count(scene.duration_us, scene.fps).unwrap(), 10);
        let move_value = scene.plan.property_value(0, 0).unwrap();
        let Value::Transform(start) = move_value else {
            panic!("expected a transform");
        };
        assert_eq!(start, Transform::IDENTITY);
        let Value::Transform(end) = scene.plan.property_value(0, 1_000_000).unwrap() else {
            panic!("expected a transform");
        };
        let decomposed = end.decompose();
        assert!((decomposed.tx - 60.0).abs() < 1e-9);
        assert!((decomposed.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        assert!(
            (scene
                .plan
                .property_value(1, 500_000)
                .unwrap()
                .as_opacity()
                .unwrap()
                - 0.8)
                .abs()
                < 1e-6
        );
        assert!(
            (scene
                .plan
                .property_value(2, 0)
                .unwrap()
                .as_color()
                .unwrap()
                .to_srgb8()[0] as i32
                - 0xc8)
                .abs()
                <= 1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn transform_components_inherit_from_the_previous_key() {
        let (dir, scene) = parsed("inherit");
        let Value::Transform(middle) = scene.plan.property_value(0, 400_000).unwrap() else {
            panic!("expected a transform");
        };
        assert!((middle.decompose().tx - 20.0).abs() < 1e-9);
        assert!((middle.decompose().rotation).abs() < 1e-9);
        let Value::Transform(last) = scene.plan.property_value(0, 1_000_000).unwrap() else {
            panic!("expected a transform");
        };
        assert!((last.decompose().tx - 60.0).abs() < 1e-9);
        assert!((last.decompose().rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layer_with_a_window_stacks_the_property() {
        let (dir, scene) = parsed("layer");
        assert!(scene.plan.property(1).unwrap().is_stacked());
        let inside = scene.plan.property_value(1, 500_000).unwrap();
        let outside = scene.plan.property_value(1, 800_000).unwrap();
        assert!((inside.as_opacity().unwrap() - 0.8).abs() < 1e-6);
        assert!((outside.as_opacity().unwrap() - 0.52).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_scripts_name_every_target_once() {
        let (dir, scene) = parsed("scripts");
        let setup = setup_script(&scene.targets);
        assert!(setup.contains("const ids = [\"box\"]"));
        assert!(setup.contains("{e:0,slot:0}"));
        assert!(setup.contains("{e:0,slot:2}"));
        let mut values = vec![Value::Number(0.0); scene.plan.len()];
        scene.plan.sample_into(0, &mut values);
        let frame = frame_script(&scene.targets, &values);
        assert!(frame.contains("setAttribute('transform','matrix(1.000000,0.000000,0.000000,1.000000,0.000000,0.000000)')"));
        assert!(frame.contains(".style.opacity='1.0000'"));
        assert!(frame.contains(".style.fill='#c82020'"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_file_is_rejected() {
        let dir = scratch("two-files");
        std::fs::write(
            dir.join("other.svg"),
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect id="b"/></svg>"##,
        )
        .unwrap();
        let scene = SCENE.replace(
            r##""kind": "svg.opacity", "src": "art.svg#box""##,
            r##""kind": "svg.opacity", "src": "other.svg#b""##,
        );
        let error = Scene::parse(&scene, &dir, "scene.json").unwrap_err();
        assert!(
            error.to_string().contains("one scene uses one drawing"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_fields_and_missing_values_name_the_place() {
        let dir = scratch("errors");
        let unknown = SCENE.replace("\"rate\": 1", "\"rate\": 1, \"speed\": 2");
        let error = Scene::parse(&unknown, &dir, "scene.json").unwrap_err();
        assert!(error.to_string().contains("scene.json.timing"));
        assert!(error.to_string().contains("unknown field \"speed\""));

        let bad_kind = SCENE.replace("svg.fill", "svg.path");
        let error = Scene::parse(&bad_kind, &dir, "scene.json").unwrap_err();
        assert!(error.to_string().contains("unknown kind svg.path"));

        let backwards = SCENE.replace("\"t\": 400", "\"t\": 2000");
        let error = Scene::parse(&backwards, &dir, "scene.json").unwrap_err();
        assert!(error.to_string().contains("time order"), "{error}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn easing_and_motion_forms_parse() {
        let spring = parse_easing(
            &serde_json::json!({"spring": {"stiffness": 400, "damping": 30, "settle": 300}}),
            "easing",
        )
        .unwrap();
        assert!(matches!(spring, Ease::Spring(_)));
        let hold = parse_easing(&serde_json::json!("hold"), "easing").unwrap();
        assert_eq!(hold, Ease::Hold);
        let steps =
            parse_easing(&serde_json::json!({"steps": 4, "at": "start"}), "easing").unwrap();
        assert!(matches!(
            steps,
            Ease::Steps {
                count: 4,
                at: StepAt::Start
            }
        ));
        let motion = parse_motion(
            &serde_json::json!({"ballistic": {"plane": 0, "restitution": 0.4}}),
            "motion",
        )
        .unwrap();
        assert!(matches!(motion, Motion::Ballistic(value) if value.restitution == 0.4));
        let arc = parse_motion(&serde_json::json!({"arc": {"bend": 0.1}}), "motion").unwrap();
        assert!(matches!(arc, Motion::Arc { .. }));
        assert!(parse_easing(&serde_json::json!({"bezier": [0.4, 0.0]}), "easing").is_err());
        assert!(parse_motion(&serde_json::json!({"ballistic": {}, "arc": {}}), "motion").is_err());
    }

    #[test]
    fn colours_accept_three_and_six_hex_digits() {
        assert_eq!(
            colour(&serde_json::json!("#abc"), "c").unwrap(),
            [0xaa, 0xbb, 0xcc]
        );
        assert_eq!(
            colour(&serde_json::json!("#0A0D12"), "c").unwrap(),
            [0x0a, 0x0d, 0x12]
        );
        assert!(colour(&serde_json::json!("abc"), "c").is_err());
        assert!(colour(&serde_json::json!("#12345"), "c").is_err());
        assert!(colour(&serde_json::json!("#gggggg"), "c").is_err());
    }

    #[test]
    fn a_duration_rounds_up_to_whole_frames() {
        assert_eq!(frame_count(0, 25).unwrap(), 2);
        assert_eq!(frame_count(1_000_000, 10).unwrap(), 10);
        assert_eq!(frame_count(1_450_000, 25).unwrap(), 37);
    }
}
