//! Paths normalised to absolute cubics, and the morph between keys.
//!
//! A pair whose subpaths pair by index and whose curve counts match morphs by
//! interpolating corresponding cubics. A pair that differs only in curve counts
//! is resampled by arc length into a common polyline, with the start offset and
//! the direction chosen to minimise travel. Prepare fails on a subpath count or
//! closed-flag mismatch; sampling fills a caller-owned buffer and does not
//! allocate once that buffer is warm.

use anikit_core::{Error, Result};

use crate::ease::{Ease, EasePlan};

/// Chords per cubic when flattening for arc-length sampling at prepare.
const FLATTEN_STEPS: usize = 24;
/// Target chord length for a resampled subpath, in path units.
const CHORD_TARGET: f64 = 4.0;
/// Fewest points a resampled subpath carries, and the most it may carry.
const MIN_POINTS: usize = 32;
const MAX_POINTS: usize = 1024;

/// A point in the path's coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    fn lerp(&self, other: &Point, t: f64) -> Point {
        Point {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
        }
    }
}

/// One cubic segment, ending at `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cubic {
    pub c1: Point,
    pub c2: Point,
    pub to: Point,
}

impl Cubic {
    fn lerp(&self, other: &Cubic, t: f64) -> Cubic {
        Cubic {
            c1: self.c1.lerp(&other.c1, t),
            c2: self.c2.lerp(&other.c2, t),
            to: self.to.lerp(&other.to, t),
        }
    }
}

/// A subpath: a start point and its cubic segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    pub start: Point,
    pub curves: Vec<Cubic>,
    pub closed: bool,
}

impl Subpath {
    pub fn new(start: Point, closed: bool) -> Subpath {
        Subpath {
            start,
            curves: Vec::new(),
            closed,
        }
    }
}

/// A path as absolute cubics, one level deep for subpaths.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Path {
    pub subpaths: Vec<Subpath>,
}

impl Path {
    pub fn new() -> Path {
        Path {
            subpaths: Vec::new(),
        }
    }

    /// Normalise absolute commands to cubics. Lines and quadratics become
    /// cubics; a close adds the closing line when the current point differs.
    pub fn from_commands(commands: &[Command]) -> Result<Path> {
        let mut path = Path::new();
        let mut current: Option<usize> = None;
        let mut point = Point::ZERO;
        for command in commands {
            match *command {
                Command::MoveTo(start) => {
                    path.subpaths.push(Subpath::new(start, false));
                    current = Some(path.subpaths.len() - 1);
                    point = start;
                }
                Command::LineTo(to) => {
                    let index = current
                        .ok_or_else(|| Error::invalid("path line starts before a move command"))?;
                    let c1 = point.lerp(&to, 1.0 / 3.0);
                    let c2 = point.lerp(&to, 2.0 / 3.0);
                    path.subpaths[index].curves.push(Cubic { c1, c2, to });
                    point = to;
                }
                Command::QuadTo { control, to } => {
                    let index = current.ok_or_else(|| {
                        Error::invalid("path quadratic starts before a move command")
                    })?;
                    let c1 = Point {
                        x: point.x + 2.0 / 3.0 * (control.x - point.x),
                        y: point.y + 2.0 / 3.0 * (control.y - point.y),
                    };
                    let c2 = Point {
                        x: to.x + 2.0 / 3.0 * (control.x - to.x),
                        y: to.y + 2.0 / 3.0 * (control.y - to.y),
                    };
                    path.subpaths[index].curves.push(Cubic { c1, c2, to });
                    point = to;
                }
                Command::CubicTo { c1, c2, to } => {
                    let index = current
                        .ok_or_else(|| Error::invalid("path cubic starts before a move command"))?;
                    path.subpaths[index].curves.push(Cubic { c1, c2, to });
                    point = to;
                }
                Command::Close => {
                    let index = current
                        .ok_or_else(|| Error::invalid("path close starts before a move command"))?;
                    let start = path.subpaths[index].start;
                    if point != start {
                        let c1 = point.lerp(&start, 1.0 / 3.0);
                        let c2 = point.lerp(&start, 2.0 / 3.0);
                        path.subpaths[index]
                            .curves
                            .push(Cubic { c1, c2, to: start });
                        point = start;
                    }
                    path.subpaths[index].closed = true;
                }
            }
        }
        Ok(path)
    }

    /// An empty path with the same shape and reserved curve buffers.
    pub fn empty_like(template: &Path) -> Path {
        Path {
            subpaths: template
                .subpaths
                .iter()
                .map(|subpath| Subpath {
                    start: Point::ZERO,
                    curves: Vec::with_capacity(subpath.curves.len()),
                    closed: subpath.closed,
                })
                .collect(),
        }
    }

    /// Fail unless the subpaths pair by index with the same closed flags. The
    /// flag returned tells whether every curve count also matches, which picks
    /// the exact morph or the resampled one.
    fn check_topology(&self, other: &Path, name: &str) -> Result<bool> {
        if self.subpaths.len() != other.subpaths.len() {
            return Err(Error::invalid(format!(
                "{name}: path subpath count {} does not match {}",
                self.subpaths.len(),
                other.subpaths.len()
            )));
        }
        let mut exact = true;
        for (index, (a, b)) in self.subpaths.iter().zip(&other.subpaths).enumerate() {
            if a.closed != b.closed {
                return Err(Error::invalid(format!(
                    "{name}: subpath {index} is closed in one key and open in the other"
                )));
            }
            if a.curves.len() != b.curves.len() {
                exact = false;
            }
        }
        Ok(exact)
    }
}

/// How a prepared path pair morphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathFidelity {
    /// Corresponding cubics interpolate and the outline is the authored one.
    Exact,
    /// The pair was resampled into a polyline at the largest sampled count.
    Resampled { points: usize },
}

/// An absolute path command before normalisation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    MoveTo(Point),
    LineTo(Point),
    QuadTo { control: Point, to: Point },
    CubicTo { c1: Point, c2: Point, to: Point },
    Close,
}

/// One path key. Easing belongs to the segment the key starts.
#[derive(Debug, Clone, PartialEq)]
pub struct PathKey {
    pub t_us: u64,
    pub path: Path,
    pub ease: Ease,
}

impl PathKey {
    pub fn new(t_us: u64, path: Path) -> PathKey {
        PathKey {
            t_us,
            path,
            ease: Ease::Linear,
        }
    }

    pub fn ease(mut self, ease: Ease) -> PathKey {
        self.ease = ease;
        self
    }

    pub fn hold(mut self) -> PathKey {
        self.ease = Ease::Hold;
        self
    }
}

/// Path keys in time order.
#[derive(Debug, Clone, PartialEq)]
pub struct PathTrack {
    pub keys: Vec<PathKey>,
}

impl PathTrack {
    pub fn new(keys: Vec<PathKey>) -> PathTrack {
        PathTrack { keys }
    }

    pub fn prepare(&self, name: &str) -> Result<PathCurve> {
        if self.keys.is_empty() {
            return Err(Error::invalid(format!(
                "{name}: a path track needs at least one key"
            )));
        }
        let mut run = 1usize;
        for i in 1..self.keys.len() {
            let previous = &self.keys[i - 1];
            let key = &self.keys[i];
            if key.t_us < previous.t_us {
                return Err(Error::invalid(format!(
                    "{name}: path keys must be in time order"
                )));
            }
            if key.t_us == previous.t_us {
                run += 1;
                if run > 2 {
                    return Err(Error::invalid(format!(
                        "{name}: three path keys share the time {}",
                        key.t_us
                    )));
                }
                if previous.ease != Ease::Hold {
                    return Err(Error::invalid(format!(
                        "{name}: two path keys share the time {} without a hold",
                        key.t_us
                    )));
                }
            } else {
                run = 1;
            }
        }
        let mut segments = Vec::with_capacity(self.keys.len().saturating_sub(1));
        let mut largest = 0usize;
        for i in 0..self.keys.len().saturating_sub(1) {
            let from = &self.keys[i];
            let to = &self.keys[i + 1];
            let span = to.t_us - from.t_us;
            let hold = span == 0 || from.ease == Ease::Hold;
            let ease = if span == 0 {
                Ease::Hold.prepare(1)?
            } else {
                from.ease.prepare(span)?
            };
            let (plan, points) = if hold {
                (SegmentPlan::Exact, 0)
            } else {
                plan_segment(&from.path, &to.path, name)?
            };
            largest = largest.max(points);
            segments.push(PathSegment {
                start_us: from.t_us,
                end_us: to.t_us,
                from: from.path.clone(),
                to: to.path.clone(),
                ease,
                plan,
                hold,
            });
        }
        let fidelity = if largest == 0 {
            PathFidelity::Exact
        } else {
            PathFidelity::Resampled { points: largest }
        };
        Ok(PathCurve {
            start_us: self.keys[0].t_us,
            end_us: self.keys[self.keys.len() - 1].t_us,
            keys: self.keys.clone(),
            segments,
            fidelity,
        })
    }
}

/// A prepared path morph. Sampling writes into a caller-owned path.
#[derive(Debug, Clone, PartialEq)]
pub struct PathCurve {
    start_us: u64,
    end_us: u64,
    keys: Vec<PathKey>,
    segments: Vec<PathSegment>,
    fidelity: PathFidelity,
}

#[derive(Debug, Clone, PartialEq)]
struct PathSegment {
    start_us: u64,
    end_us: u64,
    from: Path,
    to: Path,
    ease: EasePlan,
    plan: SegmentPlan,
    hold: bool,
}

/// How one segment moves between its two keys.
#[derive(Debug, Clone, PartialEq)]
enum SegmentPlan {
    /// Corresponding cubics interpolate.
    Exact,
    /// Both keys were sampled into point lists of equal length, already
    /// rotated into the chosen direction and start offset.
    Resampled { subpaths: Vec<ResampledSubpath> },
}

#[derive(Debug, Clone, PartialEq)]
struct ResampledSubpath {
    from: Vec<Point>,
    to: Vec<Point>,
    closed: bool,
}

impl PathCurve {
    pub fn start_us(&self) -> u64 {
        self.start_us
    }

    pub fn end_us(&self) -> u64 {
        self.end_us
    }

    /// The best fidelity across the track's segments.
    pub fn fidelity(&self) -> PathFidelity {
        self.fidelity
    }

    /// Write the path at a time into `out`. Reuses the buffers already in `out`.
    pub fn sample_into(&self, t_us: u64, out: &mut Path) {
        if t_us <= self.start_us {
            copy_path(&self.keys[0].path, out);
            return;
        }
        if t_us >= self.end_us {
            copy_path(&self.keys[self.keys.len() - 1].path, out);
            return;
        }
        let index = self.keys.partition_point(|key| key.t_us <= t_us) - 1;
        let key = &self.keys[index];
        if key.t_us == t_us {
            copy_path(&key.path, out);
            return;
        }
        let segment = &self.segments[index];
        if segment.hold {
            copy_path(&segment.from, out);
            return;
        }
        let span = (segment.end_us - segment.start_us) as f64;
        let u = (t_us - segment.start_us) as f64 / span;
        let progress = segment.ease.eval(u);
        match &segment.plan {
            SegmentPlan::Exact => interpolate(&segment.from, &segment.to, progress, out),
            SegmentPlan::Resampled { subpaths } => interpolate_resampled(subpaths, progress, out),
        }
    }
}

/// Decide how one key pair morphs. Topology is checked here, and a curve count
/// difference selects the resampled plan.
fn plan_segment(from: &Path, to: &Path, name: &str) -> Result<(SegmentPlan, usize)> {
    if from.check_topology(to, name)? {
        return Ok((SegmentPlan::Exact, 0));
    }
    let mut subpaths = Vec::with_capacity(from.subpaths.len());
    let mut largest = 0usize;
    for (a, b) in from.subpaths.iter().zip(&to.subpaths) {
        let count = point_count(a.perimeter().max(b.perimeter()));
        largest = largest.max(count);
        subpaths.push(ResampledSubpath {
            from: a.resample(count),
            to: b.resample(count),
            closed: a.closed,
        });
    }
    // A path with an open subpath is never reversed: the ends of a stroke are
    // placed on purpose, and reversing one ring of a compound path would fill
    // a hole under nonzero. Ring classification is a later step.
    if from.subpaths.iter().all(|subpath| subpath.closed) {
        let direct = travel(&subpaths, false);
        let reversed = travel(&subpaths, true);
        if reversed < direct {
            for subpath in &mut subpaths {
                subpath.to.reverse();
            }
        }
    }
    for subpath in &mut subpaths {
        if subpath.closed {
            let (offset, _) = best_offset(&subpath.from, &subpath.to, false);
            subpath.to.rotate_left(offset);
        }
    }
    Ok((SegmentPlan::Resampled { subpaths }, largest))
}

/// The point count for a resampled subpath: a 4 px chord, clamped.
fn point_count(perimeter: f64) -> usize {
    let wanted = (perimeter / CHORD_TARGET).ceil();
    if !wanted.is_finite() {
        return MAX_POINTS;
    }
    (wanted as usize).clamp(MIN_POINTS, MAX_POINTS)
}

impl Subpath {
    /// The outline flattened into chords, with cumulative length per point.
    fn flatten(&self) -> (Vec<Point>, Vec<f64>) {
        let mut points = Vec::with_capacity(self.curves.len() * FLATTEN_STEPS + 1);
        let mut lengths = Vec::with_capacity(points.capacity());
        points.push(self.start);
        lengths.push(0.0);
        let mut end = self.start;
        for curve in &self.curves {
            let start = end;
            for step in 1..=FLATTEN_STEPS {
                let t = step as f64 / FLATTEN_STEPS as f64;
                let point = cubic_point(&start, curve, t);
                let last = lengths[lengths.len() - 1];
                lengths.push(last + distance(&end, &point));
                points.push(point);
                end = point;
            }
        }
        (points, lengths)
    }

    /// The outline length along its chords.
    fn perimeter(&self) -> f64 {
        let (_, lengths) = self.flatten();
        *lengths.last().unwrap_or(&0.0)
    }

    /// Sample the outline by arc length into `count` points. A closed subpath
    /// does not repeat its start at the end; an open one keeps both ends.
    fn resample(&self, count: usize) -> Vec<Point> {
        let (points, lengths) = self.flatten();
        let total = *lengths.last().unwrap_or(&0.0);
        if total <= 0.0 {
            return vec![self.start; count];
        }
        let steps = if self.closed { count } else { count - 1 };
        let mut out = Vec::with_capacity(count);
        let mut segment = 0usize;
        for index in 0..count {
            let target = total * index as f64 / steps as f64;
            while segment + 1 < lengths.len() && lengths[segment + 1] < target {
                segment += 1;
            }
            if segment + 1 >= lengths.len() {
                out.push(points[points.len() - 1]);
                continue;
            }
            let span = lengths[segment + 1] - lengths[segment];
            let t = if span > 0.0 {
                (target - lengths[segment]) / span
            } else {
                0.0
            };
            out.push(points[segment].lerp(&points[segment + 1], t));
        }
        out
    }
}

fn cubic_point(start: &Point, curve: &Cubic, t: f64) -> Point {
    let u = 1.0 - t;
    let w0 = u * u * u;
    let w1 = 3.0 * u * u * t;
    let w2 = 3.0 * u * t * t;
    let w3 = t * t * t;
    Point {
        x: start.x * w0 + curve.c1.x * w1 + curve.c2.x * w2 + curve.to.x * w3,
        y: start.y * w0 + curve.c1.y * w1 + curve.c2.y * w2 + curve.to.y * w3,
    }
}

/// The start offset with the least squared travel, and that cost. With
/// `reversed`, the target ring is walked the other way.
fn best_offset(from: &[Point], to: &[Point], reversed: bool) -> (usize, f64) {
    let count = from.len();
    let mut best = (0usize, f64::INFINITY);
    for offset in 0..count {
        let mut cost = 0.0;
        for (index, point) in from.iter().enumerate() {
            let target = if reversed {
                (count - 1 - index + offset) % count
            } else {
                (index + offset) % count
            };
            cost += distance_squared(point, &to[target]);
        }
        if cost < best.1 {
            best = (offset, cost);
        }
    }
    best
}

/// Total squared travel of a plan, allowing a start offset per closed subpath.
fn travel(subpaths: &[ResampledSubpath], reversed: bool) -> f64 {
    let mut total = 0.0;
    for subpath in subpaths {
        if subpath.closed {
            total += best_offset(&subpath.from, &subpath.to, reversed).1;
        } else {
            for (from, to) in subpath.from.iter().zip(&subpath.to) {
                total += distance_squared(from, to);
            }
        }
    }
    total
}

fn distance(from: &Point, to: &Point) -> f64 {
    (from.x - to.x).hypot(from.y - to.y)
}

fn distance_squared(from: &Point, to: &Point) -> f64 {
    let dx = from.x - to.x;
    let dy = from.y - to.y;
    dx * dx + dy * dy
}

fn copy_path(path: &Path, out: &mut Path) {
    out.subpaths.truncate(path.subpaths.len());
    for (index, source) in path.subpaths.iter().enumerate() {
        if index == out.subpaths.len() {
            out.subpaths.push(Subpath {
                start: Point::ZERO,
                curves: Vec::with_capacity(source.curves.len()),
                closed: false,
            });
        }
        let slot = &mut out.subpaths[index];
        slot.start = source.start;
        slot.closed = source.closed;
        slot.curves.clear();
        slot.curves.extend_from_slice(&source.curves);
    }
}

fn interpolate(from: &Path, to: &Path, progress: f64, out: &mut Path) {
    out.subpaths.truncate(from.subpaths.len());
    for (index, (a, b)) in from.subpaths.iter().zip(&to.subpaths).enumerate() {
        if index == out.subpaths.len() {
            out.subpaths.push(Subpath {
                start: Point::ZERO,
                curves: Vec::with_capacity(a.curves.len()),
                closed: false,
            });
        }
        let slot = &mut out.subpaths[index];
        slot.start = a.start.lerp(&b.start, progress);
        slot.closed = a.closed;
        slot.curves.clear();
        for (ca, cb) in a.curves.iter().zip(&b.curves) {
            slot.curves.push(ca.lerp(cb, progress));
        }
    }
}

fn interpolate_resampled(subpaths: &[ResampledSubpath], progress: f64, out: &mut Path) {
    out.subpaths.truncate(subpaths.len());
    for (index, subpath) in subpaths.iter().enumerate() {
        if index == out.subpaths.len() {
            out.subpaths.push(Subpath::new(Point::ZERO, subpath.closed));
        }
        let slot = &mut out.subpaths[index];
        let count = subpath.from.len();
        slot.start = subpath.from[0].lerp(&subpath.to[0], progress);
        slot.closed = subpath.closed;
        slot.curves.clear();
        slot.curves.reserve(count);
        let chords = if subpath.closed { count } else { count - 1 };
        for (chord_index, (from, to)) in subpath
            .from
            .iter()
            .zip(&subpath.to)
            .enumerate()
            .take(chords)
        {
            let next = (chord_index + 1) % count;
            let start = from.lerp(to, progress);
            let end = subpath.from[next].lerp(&subpath.to[next], progress);
            slot.curves.push(straight(start, end));
        }
    }
}

fn straight(from: Point, to: Point) -> Cubic {
    Cubic {
        c1: from.lerp(&to, 1.0 / 3.0),
        c2: from.lerp(&to, 2.0 / 3.0),
        to,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(points: &[(f64, f64)]) -> Path {
        let mut commands = vec![Command::MoveTo(Point::new(points[0].0, points[0].1))];
        for point in &points[1..] {
            commands.push(Command::LineTo(Point::new(point.0, point.1)));
        }
        Path::from_commands(&commands).unwrap()
    }

    /// A circle of `segments` cubics at the origin, started at an angle.
    fn ring(radius: f64, segments: usize, start: f64, ccw: bool) -> Path {
        use std::f64::consts::TAU;
        let sweep = if ccw { TAU } else { -TAU };
        let step = sweep / segments as f64;
        let k = 4.0 / 3.0 * (step.abs() / 4.0).tan();
        let sign = step.signum();
        let first = Point::new(radius * start.cos(), radius * start.sin());
        let mut commands = Vec::with_capacity(segments + 2);
        commands.push(Command::MoveTo(first));
        let mut angle = start;
        let mut point = first;
        for _ in 0..segments {
            let next_angle = angle + step;
            let next = Point::new(radius * next_angle.cos(), radius * next_angle.sin());
            let tangent = |a: f64| Point::new(-a.sin() * sign, a.cos() * sign);
            let unit = k * radius;
            let c1 = Point::new(
                point.x + unit * tangent(angle).x,
                point.y + unit * tangent(angle).y,
            );
            let c2 = Point::new(
                next.x - unit * tangent(next_angle).x,
                next.y - unit * tangent(next_angle).y,
            );
            commands.push(Command::CubicTo { c1, c2, to: next });
            angle = next_angle;
            point = next;
        }
        commands.push(Command::Close);
        Path::from_commands(&commands).unwrap()
    }

    fn radii(path: &Path) -> Vec<f64> {
        let mut out = Vec::new();
        for subpath in &path.subpaths {
            let points =
                std::iter::once(&subpath.start).chain(subpath.curves.iter().map(|curve| &curve.to));
            for point in points {
                out.push(point.x.hypot(point.y));
            }
        }
        out
    }

    #[test]
    fn lines_and_quadratics_become_cubics() {
        let path = Path::from_commands(&[
            Command::MoveTo(Point::new(0.0, 0.0)),
            Command::LineTo(Point::new(30.0, 0.0)),
            Command::QuadTo {
                control: Point::new(30.0, 30.0),
                to: Point::new(0.0, 30.0),
            },
            Command::Close,
        ])
        .unwrap();
        assert_eq!(path.subpaths.len(), 1);
        assert_eq!(path.subpaths[0].curves.len(), 3);
        assert!(path.subpaths[0].closed);
        let first = path.subpaths[0].curves[0];
        assert_eq!(first.c1, Point::new(10.0, 0.0));
        assert_eq!(first.to, Point::new(30.0, 0.0));
    }

    #[test]
    fn a_matching_path_morph_keeps_both_endpoints_exact() {
        let a = path(&[(0.0, 0.0), (100.0, 0.0)]);
        let b = path(&[(0.0, 100.0), (100.0, 100.0)]);
        let curve = PathTrack::new(vec![
            PathKey::new(0, a.clone()),
            PathKey::new(1_000_000, b.clone()),
        ])
        .prepare("logo")
        .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(0, &mut out);
        assert_eq!(out, a);
        curve.sample_into(1_000_000, &mut out);
        assert_eq!(out, b);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out.subpaths[0].curves[0].to, Point::new(100.0, 50.0));
    }

    #[test]
    fn a_curve_count_mismatch_resamples_instead_of_failing() {
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 3, 0.0, true);
        let curve = PathTrack::new(vec![
            PathKey::new(0, a.clone()),
            PathKey::new(1_000_000, b.clone()),
        ])
        .prepare("logo")
        .unwrap();
        match curve.fidelity() {
            PathFidelity::Resampled { points } => {
                assert!(
                    (MIN_POINTS..=MAX_POINTS).contains(&points),
                    "points {points}"
                );
            }
            other => panic!("expected a resampled plan, got {other:?}"),
        }
        let mut out = Path::empty_like(&a);
        curve.sample_into(0, &mut out);
        assert_eq!(out, a);
        curve.sample_into(1_000_000, &mut out);
        assert_eq!(out, b);
    }

    #[test]
    fn a_matched_pair_reports_exact() {
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 4, 0.0, true);
        let curve = PathTrack::new(vec![PathKey::new(0, a), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        assert_eq!(curve.fidelity(), PathFidelity::Exact);
    }

    #[test]
    fn a_rotated_start_is_aligned_before_the_morph() {
        use std::f64::consts::PI;
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 6, PI, true);
        let curve = PathTrack::new(vec![PathKey::new(0, a.clone()), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        for radius in radii(&out) {
            assert!((radius - 100.0).abs() < 6.0, "radius {radius}");
        }
    }

    #[test]
    fn an_opposite_winding_is_reversed_before_the_morph() {
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 6, 0.0, false);
        let curve = PathTrack::new(vec![PathKey::new(0, a.clone()), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        let min = radii(&out).into_iter().fold(f64::INFINITY, f64::min);
        assert!(min > 95.0, "mid-morph pinched to {min}");
    }

    #[test]
    fn an_open_subpath_keeps_its_authored_ends() {
        let a = path(&[(0.0, 0.0), (100.0, 0.0)]);
        let b = path(&[(0.0, 100.0), (50.0, 100.0), (100.0, 100.0)]);
        let curve = PathTrack::new(vec![PathKey::new(0, a.clone()), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out.subpaths[0].start, Point::new(0.0, 50.0));
        let end = out.subpaths[0].curves.last().unwrap().to;
        assert_eq!(end, Point::new(100.0, 50.0));
    }

    #[test]
    fn a_zero_length_subpath_resamples_to_its_own_point() {
        let dot = Path::from_commands(&[Command::MoveTo(Point::new(10.0, 10.0)), Command::Close])
            .unwrap();
        let circle = ring(100.0, 4, 0.0, true);
        let curve = PathTrack::new(vec![
            PathKey::new(0, dot.clone()),
            PathKey::new(1_000_000, circle),
        ])
        .prepare("logo")
        .unwrap();
        let mut out = Path::empty_like(&dot);
        curve.sample_into(500_000, &mut out);
        for subpath in &out.subpaths {
            assert!(subpath.start.x.is_finite() && subpath.start.y.is_finite());
            for point in subpath.curves.iter().map(|curve| curve.to) {
                assert!(point.x.is_finite() && point.y.is_finite());
            }
        }
    }

    #[test]
    fn a_closed_flag_mismatch_still_fails() {
        let a = ring(100.0, 4, 0.0, true);
        let mut b = ring(100.0, 4, 0.0, true);
        b.subpaths[0].closed = false;
        let error = PathTrack::new(vec![PathKey::new(0, a), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap_err()
            .to_string();
        assert!(error.contains("closed"), "{error}");
    }

    #[test]
    fn a_hold_between_different_shapes_keeps_the_first() {
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 3, 0.0, true);
        let curve = PathTrack::new(vec![
            PathKey::new(0, a.clone()).hold(),
            PathKey::new(1_000_000, b.clone()),
        ])
        .prepare("logo")
        .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out, a);
        curve.sample_into(1_000_000, &mut out);
        assert_eq!(out, b);
    }

    #[test]
    fn a_resampled_morph_reuses_a_warm_buffer() {
        let a = ring(100.0, 4, 0.0, true);
        let b = ring(100.0, 3, 0.0, true);
        let curve = PathTrack::new(vec![PathKey::new(0, a.clone()), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        let capacity = out.subpaths[0].curves.capacity();
        assert!(capacity >= MIN_POINTS, "capacity {capacity}");
        for t in 0..1000 {
            curve.sample_into(t * 1000, &mut out);
        }
        assert_eq!(out.subpaths[0].curves.capacity(), capacity);
    }

    #[test]
    fn mismatched_subpath_counts_fail() {
        let a = Path::from_commands(&[
            Command::MoveTo(Point::new(0.0, 0.0)),
            Command::LineTo(Point::new(10.0, 0.0)),
            Command::MoveTo(Point::new(0.0, 10.0)),
            Command::LineTo(Point::new(10.0, 10.0)),
        ])
        .unwrap();
        let b = path(&[(0.0, 0.0), (10.0, 0.0)]);
        let error = PathTrack::new(vec![PathKey::new(0, a), PathKey::new(1, b)])
            .prepare("logo")
            .unwrap_err()
            .to_string();
        assert!(error.contains("subpath count 2"), "{error}");
        assert!(error.contains('1'), "{error}");
    }

    #[test]
    fn a_path_hold_jumps_at_the_next_key() {
        let a = path(&[(0.0, 0.0), (10.0, 0.0)]);
        let b = path(&[(0.0, 100.0), (10.0, 100.0)]);
        let curve = PathTrack::new(vec![
            PathKey::new(0, a.clone()).hold(),
            PathKey::new(500_000, b.clone()),
        ])
        .prepare("logo")
        .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(499_999, &mut out);
        assert_eq!(out, a);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out, b);
    }

    #[test]
    fn the_sampler_reuses_a_warm_buffer() {
        let a = path(&[(0.0, 0.0), (100.0, 0.0)]);
        let b = path(&[(0.0, 100.0), (100.0, 100.0)]);
        let curve = PathTrack::new(vec![PathKey::new(0, a.clone()), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap();
        let mut out = Path::empty_like(&a);
        curve.sample_into(500_000, &mut out);
        let capacity = out.subpaths[0].curves.capacity();
        for t in 0..1000 {
            curve.sample_into(t * 1000, &mut out);
        }
        assert_eq!(out.subpaths[0].curves.capacity(), capacity);
    }
}
