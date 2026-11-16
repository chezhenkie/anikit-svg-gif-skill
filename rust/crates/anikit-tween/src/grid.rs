//! ASCII grids: a density field per cell plus colour, interpolated before a
//! glyph is chosen. Glyph identity is not interpolable, so the crate never
//! carries it through a morph.

use anikit_core::{Error, Result};

use crate::ease::{Ease, EasePlan};
use crate::track::Lerp;
use crate::value::Color;

/// One grid cell before the ramp is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub density: f32,
    pub color: Color,
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        density: 0.0,
        color: Color::TRANSPARENT,
    };
}

/// A fixed grid of cells. Motion below one cell is impossible.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
}

impl Grid {
    pub fn new(cols: u16, rows: u16, fill: Cell) -> Grid {
        Grid {
            cols,
            rows,
            cells: vec![fill; cols as usize * rows as usize],
        }
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// An empty grid with the same dimensions and reserved cells.
    pub fn empty_like(template: &Grid) -> Grid {
        Grid {
            cols: template.cols,
            rows: template.rows,
            cells: vec![Cell::EMPTY; template.cells.len()],
        }
    }

    fn check(&self, name: &str) -> Result<()> {
        if self.cells.len() != self.cols as usize * self.rows as usize {
            return Err(Error::invalid(format!(
                "{name}: grid is {} by {} but holds {} cells",
                self.cols,
                self.rows,
                self.cells.len()
            )));
        }
        Ok(())
    }
}

/// One grid key. Easing belongs to the segment the key starts.
#[derive(Debug, Clone, PartialEq)]
pub struct GridKey {
    pub t_us: u64,
    pub grid: Grid,
    pub ease: Ease,
}

impl GridKey {
    pub fn new(t_us: u64, grid: Grid) -> GridKey {
        GridKey {
            t_us,
            grid,
            ease: Ease::Linear,
        }
    }

    pub fn ease(mut self, ease: Ease) -> GridKey {
        self.ease = ease;
        self
    }

    pub fn hold(mut self) -> GridKey {
        self.ease = Ease::Hold;
        self
    }
}

/// Grid keys in time order.
#[derive(Debug, Clone, PartialEq)]
pub struct GridTrack {
    pub keys: Vec<GridKey>,
}

impl GridTrack {
    pub fn new(keys: Vec<GridKey>) -> GridTrack {
        GridTrack { keys }
    }

    pub fn prepare(&self, name: &str) -> Result<GridCurve> {
        if self.keys.is_empty() {
            return Err(Error::invalid(format!(
                "{name}: a grid track needs at least one key"
            )));
        }
        let mut run = 1usize;
        for i in 1..self.keys.len() {
            let previous = &self.keys[i - 1];
            let key = &self.keys[i];
            if key.t_us < previous.t_us {
                return Err(Error::invalid(format!(
                    "{name}: grid keys must be in time order"
                )));
            }
            if key.t_us == previous.t_us {
                run += 1;
                if run > 2 {
                    return Err(Error::invalid(format!(
                        "{name}: three grid keys share the time {}",
                        key.t_us
                    )));
                }
                if previous.ease != Ease::Hold {
                    return Err(Error::invalid(format!(
                        "{name}: two grid keys share the time {} without a hold",
                        key.t_us
                    )));
                }
            } else {
                run = 1;
            }
        }
        let first = &self.keys[0].grid;
        first.check(name)?;
        for key in &self.keys {
            key.grid.check(name)?;
            if key.grid.cols != first.cols || key.grid.rows != first.rows {
                return Err(Error::invalid(format!(
                    "{name}: grid sizes {}x{} and {}x{} differ",
                    first.cols, first.rows, key.grid.cols, key.grid.rows
                )));
            }
        }
        let mut segments = Vec::with_capacity(self.keys.len().saturating_sub(1));
        for i in 0..self.keys.len().saturating_sub(1) {
            let from = &self.keys[i];
            let to = &self.keys[i + 1];
            let span = to.t_us - from.t_us;
            let ease = if from.ease == Ease::Hold {
                Ease::Hold.prepare(1)?
            } else {
                from.ease.prepare(span)?
            };
            segments.push(GridSegment {
                start_us: from.t_us,
                end_us: to.t_us,
                from: from.grid.clone(),
                to: to.grid.clone(),
                ease,
            });
        }
        Ok(GridCurve {
            start_us: self.keys[0].t_us,
            end_us: self.keys[self.keys.len() - 1].t_us,
            keys: self.keys.clone(),
            segments,
        })
    }
}

/// A prepared grid track. Sampling writes into a caller-owned grid.
#[derive(Debug, Clone, PartialEq)]
pub struct GridCurve {
    start_us: u64,
    end_us: u64,
    keys: Vec<GridKey>,
    segments: Vec<GridSegment>,
}

#[derive(Debug, Clone, PartialEq)]
struct GridSegment {
    start_us: u64,
    end_us: u64,
    from: Grid,
    to: Grid,
    ease: EasePlan,
}

impl GridCurve {
    pub fn start_us(&self) -> u64 {
        self.start_us
    }

    pub fn end_us(&self) -> u64 {
        self.end_us
    }

    pub fn sample_into(&self, t_us: u64, out: &mut Grid) {
        if t_us <= self.start_us {
            copy_grid(&self.keys[0].grid, out);
            return;
        }
        if t_us >= self.end_us {
            copy_grid(&self.keys[self.keys.len() - 1].grid, out);
            return;
        }
        let index = self.keys.partition_point(|key| key.t_us <= t_us) - 1;
        let key = &self.keys[index];
        if key.t_us == t_us {
            copy_grid(&key.grid, out);
            return;
        }
        let segment = &self.segments[index];
        let span = (segment.end_us - segment.start_us) as f64;
        let u = (t_us - segment.start_us) as f64 / span;
        let progress = segment.ease.eval(u);
        resize(out, &segment.from);
        for (slot, (a, b)) in out
            .cells
            .iter_mut()
            .zip(segment.from.cells.iter().zip(&segment.to.cells))
        {
            slot.density =
                (a.density as f64 + (b.density as f64 - a.density as f64) * progress) as f32;
            slot.color = a.color.lerp(&b.color, progress);
        }
    }
}

fn copy_grid(grid: &Grid, out: &mut Grid) {
    resize(out, grid);
    out.cells.copy_from_slice(&grid.cells);
}

fn resize(out: &mut Grid, template: &Grid) {
    out.cols = template.cols;
    out.rows = template.rows;
    if out.cells.len() != template.cells.len() {
        out.cells.resize(template.cells.len(), Cell::EMPTY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Color;

    fn grid(density: f32, color: Color) -> Grid {
        Grid::new(2, 1, Cell { density, color })
    }

    #[test]
    fn a_density_field_reaches_its_endpoints_exactly() {
        let a = grid(0.0, Color::linear(0.0, 0.0, 0.0, 1.0));
        let b = grid(1.0, Color::linear(1.0, 1.0, 1.0, 1.0));
        let curve = GridTrack::new(vec![
            GridKey::new(0, a.clone()),
            GridKey::new(1_000_000, b.clone()),
        ])
        .prepare("cells")
        .unwrap();
        let mut out = Grid::empty_like(&a);
        curve.sample_into(0, &mut out);
        assert_eq!(out.cells[0].density, 0.0);
        curve.sample_into(1_000_000, &mut out);
        assert_eq!(out.cells[0].density, 1.0);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out.cells[0].density, 0.5);
        assert!((out.cells[0].color.r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn grid_sizes_must_match() {
        let a = grid(0.0, Color::TRANSPARENT);
        let b = Grid::new(3, 1, Cell::EMPTY);
        let error = GridTrack::new(vec![GridKey::new(0, a), GridKey::new(1, b)])
            .prepare("cells")
            .unwrap_err()
            .to_string();
        assert!(error.contains("2x1") && error.contains("3x1"), "{error}");
    }

    #[test]
    fn a_grid_hold_jumps_at_the_next_key() {
        let a = grid(0.0, Color::TRANSPARENT);
        let b = grid(1.0, Color::TRANSPARENT);
        let curve = GridTrack::new(vec![
            GridKey::new(0, a.clone()).hold(),
            GridKey::new(500_000, b.clone()),
        ])
        .prepare("cells")
        .unwrap();
        let mut out = Grid::empty_like(&a);
        curve.sample_into(499_999, &mut out);
        assert_eq!(out.cells[0].density, 0.0);
        curve.sample_into(500_000, &mut out);
        assert_eq!(out.cells[0].density, 1.0);
    }
}
