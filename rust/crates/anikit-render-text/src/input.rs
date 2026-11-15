//! Loading text frames from a file or a directory of `.txt` files.

use std::path::Path;

use anikit_core::{Error, Result};

use crate::{Cell, TextFrame, sgr};

/// Read frames from a text file with `---` separators, or from every `.txt`
/// file in a directory, sorted by name.
pub fn load_frames(path: &Path, default_delay: u32, default_fg: [u8; 3]) -> Result<Vec<TextFrame>> {
    if path.is_dir() {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(path)
            .map_err(|err| Error::io(format!("cannot list {}: {err}", path.display())))?
        {
            let entry =
                entry.map_err(|err| Error::io(format!("cannot list {}: {err}", path.display())))?;
            let file = entry.path();
            let is_text = file
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("txt"))
                .unwrap_or(false);
            if !is_text || !file.is_file() {
                continue;
            }
            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((name, file));
        }
        if files.is_empty() {
            return Err(Error::invalid(format!(
                "no .txt frames in {}",
                path.display()
            )));
        }
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut frames = Vec::with_capacity(files.len());
        for (_, file) in files {
            let text = std::fs::read_to_string(&file)
                .map_err(|err| Error::io(format!("cannot read {}: {err}", file.display())))?;
            frames.push(TextFrame {
                lines: parse_lines(&text, default_fg),
                delay_ms: default_delay,
            });
        }
        return Ok(frames);
    }
    if !path.is_file() {
        return Err(Error::invalid(format!(
            "input not found: {}",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|err| Error::io(format!("cannot read {}: {err}", path.display())))?;
    Ok(parse_frames(&text, default_delay, default_fg))
}

/// Split one text document into frames.
pub fn parse_frames(text: &str, default_delay: u32, default_fg: [u8; 3]) -> Vec<TextFrame> {
    let mut frames = Vec::new();
    let mut lines: Vec<Vec<Cell>> = Vec::new();
    let mut delay = default_delay;
    for raw in text.lines() {
        if let Some(next) = separator(raw) {
            frames.push(TextFrame {
                lines: std::mem::take(&mut lines),
                delay_ms: delay,
            });
            delay = next.unwrap_or(default_delay);
        } else {
            lines.push(sgr::parse_line(raw, default_fg));
        }
    }
    if !lines.is_empty() || frames.is_empty() {
        frames.push(TextFrame {
            lines,
            delay_ms: delay,
        });
    }
    frames
}

fn parse_lines(text: &str, default_fg: [u8; 3]) -> Vec<Vec<Cell>> {
    text.lines()
        .map(|line| sgr::parse_line(line, default_fg))
        .collect()
}

/// `---` or `--- 250`, and nothing else on the line.
fn separator(line: &str) -> Option<Option<u32>> {
    let rest = line.strip_prefix("---")?;
    if rest.is_empty() {
        return Some(None);
    }
    if !rest.starts_with(|ch: char| ch.is_whitespace()) {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return Some(None);
    }
    if rest.bytes().all(|b| b.is_ascii_digit()) {
        return rest.parse().ok().map(Some);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: [u8; 3] = [230, 237, 246];

    #[test]
    fn separators_split_frames_and_carry_delays() {
        let frames = parse_frames("aa\n--- 250\nbb\n---\ncc\n", 100, FG);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].delay_ms, 100);
        assert_eq!(frames[0].lines.len(), 1);
        assert_eq!(frames[1].delay_ms, 250);
        assert_eq!(frames[2].delay_ms, 100);
        assert_eq!(frames[2].lines[0][0].ch, 'c');
    }

    #[test]
    fn a_separator_only_file_gives_one_empty_frame() {
        let frames = parse_frames("--- 200\n", 100, FG);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].lines.is_empty());
    }

    #[test]
    fn a_dash_line_that_is_not_a_separator_is_text() {
        let frames = parse_frames("---x\n", 100, FG);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].lines[0].len(), 4);
    }

    #[test]
    fn a_trailing_separator_adds_no_empty_frame() {
        let frames = parse_frames("aa\n---\n", 100, FG);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn an_empty_document_gives_one_empty_frame() {
        let frames = parse_frames("", 100, FG);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].lines.is_empty());
    }

    #[test]
    fn colours_apply_while_loading() {
        let frames = parse_frames("\x1b[32mok\n", 100, FG);
        assert_eq!(frames[0].lines[0][0].fg, [13, 188, 121]);
    }

    #[test]
    fn a_directory_gives_one_frame_per_text_file() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/text-directory");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("b.txt"), "second\n").unwrap();
        std::fs::write(dir.join("a.txt"), "first\n").unwrap();
        std::fs::write(dir.join("skip.md"), "ignored\n").unwrap();
        let frames = load_frames(&dir, 100, FG).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].lines[0][0].ch, 'f');
        assert_eq!(frames[1].lines[0][0].ch, 's');
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_without_text_files_is_an_error() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-tmp/text-directory-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("frame.png"), b"not text").unwrap();
        let err = load_frames(&dir, 100, FG).unwrap_err();
        assert!(err.to_string().contains("no .txt frames"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
