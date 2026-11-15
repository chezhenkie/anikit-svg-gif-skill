//! Collapsing identical consecutive frames into one longer hold.

use crate::Frame;

/// One run: the index of the frame that represents it, and its summed delay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub frame: usize,
    pub delay_ms: u32,
}

/// Merge identical consecutive frames. The first frame of a run is kept.
pub fn merge_runs(frames: &[Frame], delays: &[u32]) -> Vec<Run> {
    debug_assert_eq!(frames.len(), delays.len());
    let mut runs: Vec<Run> = Vec::with_capacity(frames.len());
    for (index, (frame, delay)) in frames.iter().zip(delays).enumerate() {
        match runs.last_mut() {
            Some(run) if frames[run.frame].same_as(frame) => run.delay_ms += *delay,
            _ => runs.push(Run {
                frame: index,
                delay_ms: *delay,
            }),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(rgb: [u8; 3]) -> Frame {
        let mut frame = Frame::new(2, 2).unwrap();
        frame.fill(rgb);
        frame
    }

    #[test]
    fn identical_neighbours_collapse() {
        let frames = vec![solid([1, 2, 3]), solid([1, 2, 3]), solid([4, 5, 6])];
        let runs = merge_runs(&frames, &[100, 100, 100]);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].delay_ms, 200);
        assert_eq!(runs[1].frame, 2);
    }

    #[test]
    fn non_adjacent_duplicates_do_not_collapse() {
        let frames = vec![solid([1, 2, 3]), solid([4, 5, 6]), solid([1, 2, 3])];
        let runs = merge_runs(&frames, &[100, 100, 100]);
        assert_eq!(runs.len(), 3);
    }
}
