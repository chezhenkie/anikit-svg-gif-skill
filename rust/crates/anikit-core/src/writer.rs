//! File plumbing shared by the writers: the temp name beside the target and
//! the readback check that runs before the temp file replaces it.

use std::path::{Path, PathBuf};

use crate::inspect::{inspect_apng_file, inspect_file};
use crate::output::{OutputFormat, OutputSummary};
use crate::{Error, Result};

/// Check the file on disk against the summary, then move it into place.
pub(crate) fn verify_and_rename(
    temp: &Path,
    path: &Path,
    summary: &OutputSummary,
    format: OutputFormat,
) -> Result<()> {
    let info = match format {
        OutputFormat::Gif => inspect_file(temp)?,
        OutputFormat::Apng => inspect_apng_file(temp)?,
        OutputFormat::Svg => {
            return Err(Error::verify("SVG files are written by the tween route"));
        }
    };
    if info.frames != summary.frames {
        return Err(Error::verify(format!(
            "frame count {} does not match {}",
            info.frames, summary.frames
        )));
    }
    if info.width != summary.width || info.height != summary.height {
        return Err(Error::verify(format!(
            "size {}x{} does not match {}x{}",
            info.width, info.height, summary.width, summary.height
        )));
    }
    if info.total_ms.abs_diff(summary.total_ms) > 10 {
        return Err(Error::verify(format!(
            "duration {} ms does not match {} ms",
            info.total_ms, summary.total_ms
        )));
    }
    std::fs::rename(temp, path)?;
    Ok(())
}

/// The temp file beside the target, e.g. `out.tmp.gif`.
pub fn temp_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("gif");
    path.with_file_name(format!("{stem}.tmp.{extension}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::GifEncoder;
    use crate::palette::Palette;
    use std::fs::File;

    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp")
            .join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_count_mismatch_is_refused_and_nothing_is_renamed() {
        let dir = scratch("writer-verify");
        let temp = dir.join("check.tmp.gif");
        let target = dir.join("check.gif");
        let palette = Palette::from_colors(vec![[0, 0, 0], [255, 255, 255]]).unwrap();
        let mut encoder = GifEncoder::new(File::create(&temp).unwrap(), 2, 2, &palette, 0).unwrap();
        encoder.write_frame(&[0, 1, 1, 0], 100).unwrap();
        let summary = encoder.finish().unwrap();

        let wrong = OutputSummary {
            frames: summary.frames + 1,
            ..summary
        };
        assert!(verify_and_rename(&temp, &target, &wrong, OutputFormat::Gif).is_err());
        assert!(!target.exists());
        assert!(temp.exists(), "a refused check keeps the temp file");

        verify_and_rename(&temp, &target, &summary, OutputFormat::Gif).unwrap();
        assert!(target.exists());
        assert!(!temp.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
