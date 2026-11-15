//! ANSI SGR colour parsing and the xterm palette.
//!
//! The frozen tool strips every CSI sequence except SGR, then reads 16, 256
//! and truecolour foreground codes. Bold and background codes are ignored, so
//! the glyph colour is the only state that survives a line.

use crate::Cell;

const BASE16: [[u8; 3]; 16] = [
    [0, 0, 0],
    [205, 49, 49],
    [13, 188, 121],
    [229, 229, 16],
    [36, 114, 200],
    [188, 63, 188],
    [17, 168, 205],
    [229, 229, 229],
    [102, 102, 102],
    [241, 76, 76],
    [35, 209, 139],
    [245, 245, 67],
    [59, 142, 234],
    [214, 112, 214],
    [41, 184, 219],
    [255, 255, 255],
];

const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// An xterm 256 colour index as RGB.
pub fn xterm_rgb(index: u8) -> [u8; 3] {
    match index {
        0..=15 => BASE16[index as usize],
        16..=231 => {
            let n = index - 16;
            [
                CUBE[(n / 36) as usize],
                CUBE[((n / 6) % 6) as usize],
                CUBE[(n % 6) as usize],
            ]
        }
        _ => {
            let v = 8 + 10 * (index - 232);
            [v, v, v]
        }
    }
}

/// One line of text into cells with colours applied.
pub fn parse_line(line: &str, default_fg: [u8; 3]) -> Vec<Cell> {
    let expanded = expand_tabs(line);
    let stripped = strip_csi(&expanded);
    let mut cells = Vec::with_capacity(stripped.len());
    let mut fg = default_fg;
    let bytes = stripped.as_bytes();
    let mut text_at = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            let mut end = i + 2;
            while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b';') {
                end += 1;
            }
            if end < bytes.len() && bytes[end] == b'm' {
                for ch in stripped[text_at..i].chars() {
                    cells.push(Cell { ch, fg });
                }
                apply_params(&stripped[i + 2..end], &mut fg, default_fg);
                i = end + 1;
                text_at = i;
                continue;
            }
        }
        i += 1;
    }
    for ch in stripped[text_at..].chars() {
        cells.push(Cell { ch, fg });
    }
    cells
}

fn apply_params(text: &str, fg: &mut [u8; 3], default_fg: [u8; 3]) {
    let mut params: Vec<u16> = text
        .split(';')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect();
    if params.is_empty() {
        params.push(0);
    }
    let mut i = 0;
    while i < params.len() {
        let code = params[i];
        match code {
            0 => *fg = default_fg,
            30..=37 => *fg = BASE16[(code - 30) as usize],
            90..=97 => *fg = BASE16[(code - 90 + 8) as usize],
            39 => *fg = default_fg,
            38 if i + 1 < params.len() => {
                let mode = params[i + 1];
                if mode == 5 && i + 2 < params.len() {
                    *fg = xterm_rgb(params[i + 2].min(255) as u8);
                    i += 2;
                } else if mode == 2 && i + 4 < params.len() {
                    *fg = [
                        params[i + 2].min(255) as u8,
                        params[i + 3].min(255) as u8,
                        params[i + 4].min(255) as u8,
                    ];
                    i += 4;
                } else {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn expand_tabs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut column = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let pad = 4 - column % 4;
            for _ in 0..pad {
                out.push(' ');
            }
            column += pad;
        } else {
            out.push(ch);
            column += 1;
        }
    }
    out
}

fn strip_csi(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'[') {
            let mut end = i + 2;
            while end < bytes.len() && matches!(bytes[end], b'0'..=b'9' | b';' | b'?') {
                end += 1;
            }
            if end < bytes.len() {
                let final_byte = bytes[end];
                let strippable = matches!(final_byte,
                    b'A'..=b'L' | b'N'..=b'Z' | b'a'..=b'l' | b'n'..=b'z');
                if strippable {
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = line[i..].chars().next().expect("index is a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FG: [u8; 3] = [230, 237, 246];

    #[test]
    fn plain_text_keeps_the_default_colour() {
        let cells = parse_line("abc", FG);
        assert_eq!(cells.len(), 3);
        assert!(cells.iter().all(|c| c.fg == FG && c.ch != ' '));
    }

    #[test]
    fn sixteen_colours_apply_from_the_code() {
        let cells = parse_line("\x1b[31mred\x1b[39mback", FG);
        assert_eq!(cells[0].fg, [205, 49, 49]);
        assert_eq!(cells[2].fg, [205, 49, 49]);
        assert_eq!(cells[3].fg, FG);
    }

    #[test]
    fn bright_colours_and_reset_work() {
        let cells = parse_line("\x1b[92mgreen\x1b[0mplain", FG);
        assert_eq!(cells[0].fg, [35, 209, 139]);
        assert_eq!(cells[5].fg, FG);
    }

    #[test]
    fn indexed_and_truecolour_apply() {
        let cells = parse_line("\x1b[38;5;196mx\x1b[38;2;1;2;3my", FG);
        assert_eq!(cells[0].fg, [255, 0, 0]);
        assert_eq!(cells[1].fg, [1, 2, 3]);
    }

    #[test]
    fn indexed_colours_cover_the_cube_and_grays() {
        assert_eq!(xterm_rgb(16), [0, 0, 0]);
        assert_eq!(xterm_rgb(196), [255, 0, 0]);
        assert_eq!(xterm_rgb(232), [8, 8, 8]);
        assert_eq!(xterm_rgb(255), [238, 238, 238]);
    }

    #[test]
    fn non_sgr_sequences_are_stripped() {
        let cells = parse_line("a\x1b[2Kb\x1b[?25lc", FG);
        assert_eq!(cells.iter().map(|c| c.ch).collect::<String>(), "abc");
    }

    #[test]
    fn bold_and_background_codes_are_ignored() {
        let cells = parse_line("\x1b[1;44mbold", FG);
        assert!(cells.iter().all(|c| c.fg == FG));
        assert_eq!(cells.len(), 4);
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        let cells = parse_line("a\tb", FG);
        assert_eq!(cells.len(), 5);
        assert_eq!(cells[1].ch, ' ');
        assert_eq!(cells[4].ch, 'b');
    }

    #[test]
    fn wide_characters_are_single_cells() {
        let cells = parse_line("\u{2588}\u{2500}", FG);
        assert_eq!(cells.len(), 2);
    }
}
