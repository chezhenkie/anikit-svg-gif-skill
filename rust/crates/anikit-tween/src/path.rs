//! Paths normalised to absolute cubics, and the morph between matching keys.
//!
//! Two paths morph only when their subpath count, per-subpath curve count and
//! closed flags match. Prepare fails with both counts otherwise; the caller can
//! ask for a dissolve instead. Sampling fills a caller-owned buffer and does
//! not allocate once that buffer is warm.

use anikit_core::{Error, Result};

use crate::ease::{Ease, EasePlan};

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

    /// Fail unless the two paths can morph by interpolating corresponding
    /// cubics. The message names both counts.
    pub fn check_match(&self, other: &Path, name: &str) -> Result<()> {
        if self.subpaths.len() != other.subpaths.len() {
            return Err(Error::invalid(format!(
                "{name}: path subpath count {} does not match {}",
                self.subpaths.len(),
                other.subpaths.len()
            )));
        }
        for (index, (a, b)) in self.subpaths.iter().zip(&other.subpaths).enumerate() {
            if a.curves.len() != b.curves.len() {
                return Err(Error::invalid(format!(
                    "{name}: subpath {index} curve count {} does not match {}",
                    a.curves.len(),
                    b.curves.len()
                )));
            }
            if a.closed != b.closed {
                return Err(Error::invalid(format!(
                    "{name}: subpath {index} is closed in one key and open in the other"
                )));
            }
        }
        Ok(())
    }
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
        for i in 0..self.keys.len().saturating_sub(1) {
            if self.keys[i].ease != Ease::Hold {
                self.keys[i]
                    .path
                    .check_match(&self.keys[i + 1].path, name)?;
            }
        }
        let mut segments = Vec::with_capacity(self.keys.len().saturating_sub(1));
        for i in 0..self.keys.len().saturating_sub(1) {
            let from = &self.keys[i];
            let to = &self.keys[i + 1];
            let span = to.t_us - from.t_us;
            let ease = if span == 0 {
                Ease::Hold.prepare(1)?
            } else {
                from.ease.prepare(span)?
            };
            segments.push(PathSegment {
                start_us: from.t_us,
                end_us: to.t_us,
                from: from.path.clone(),
                to: to.path.clone(),
                ease,
            });
        }
        Ok(PathCurve {
            start_us: self.keys[0].t_us,
            end_us: self.keys[self.keys.len() - 1].t_us,
            keys: self.keys.clone(),
            segments,
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
}

#[derive(Debug, Clone, PartialEq)]
struct PathSegment {
    start_us: u64,
    end_us: u64,
    from: Path,
    to: Path,
    ease: EasePlan,
}

impl PathCurve {
    pub fn start_us(&self) -> u64 {
        self.start_us
    }

    pub fn end_us(&self) -> u64 {
        self.end_us
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
        let span = (segment.end_us - segment.start_us) as f64;
        let u = (t_us - segment.start_us) as f64 / span;
        let progress = segment.ease.eval(u);
        interpolate(&segment.from, &segment.to, progress, out);
    }
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
    fn mismatched_curve_counts_fail_with_both_counts() {
        let a = path(&[(0.0, 0.0), (100.0, 0.0)]);
        let b = path(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]);
        let error = PathTrack::new(vec![PathKey::new(0, a), PathKey::new(1_000_000, b)])
            .prepare("logo")
            .unwrap_err()
            .to_string();
        assert!(error.contains('1') && error.contains('2'), "{error}");
        assert!(error.contains("subpath"), "{error}");
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
