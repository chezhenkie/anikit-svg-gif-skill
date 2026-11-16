//! Decoding a GIF back to composited canvases, for tests.
//!
//! The writer emits one changed rectangle per frame with disposal 1, so a
//! decoded frame is a patch, not a picture. A pixel comparison has to composite
//! the patches the way a viewer does. Test targets include this file by path so
//! the helper exists once.

#![allow(dead_code)]

use std::path::Path;

/// One decoded frame: its delay and the patch it draws.
pub struct Patch {
    pub delay_ms: u32,
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
}

/// The canvas size and every frame as the file stores it.
pub fn patches(path: &Path) -> (u16, u16, Vec<Patch>) {
    let bytes = std::fs::read(path).expect("read gif");
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(&bytes[..]).expect("gif header");
    let (width, height) = (decoder.width(), decoder.height());
    let mut frames = Vec::new();
    while let Some(frame) = decoder.read_next_frame().expect("gif frame") {
        frames.push(Patch {
            delay_ms: frame.delay as u32 * 10,
            left: frame.left,
            top: frame.top,
            width: frame.width,
            height: frame.height,
            rgba: frame.buffer.to_vec(),
        });
    }
    (width, height, frames)
}

/// A canvas for every frame, composited in order.
pub fn canvases(path: &Path) -> (u16, u16, Vec<(Vec<u8>, u32)>) {
    let (width, height, patches) = patches(path);
    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    let mut out = Vec::with_capacity(patches.len());
    for patch in patches {
        let row_bytes = patch.width as usize * 4;
        for row in 0..patch.height as usize {
            let src = row * row_bytes;
            let dst = ((patch.top as usize + row) * width as usize + patch.left as usize) * 4;
            canvas[dst..dst + row_bytes].copy_from_slice(&patch.rgba[src..src + row_bytes]);
        }
        out.push((canvas.clone(), patch.delay_ms));
    }
    (width, height, out)
}

/// FNV-1a 64 over the decoded RGBA canvas, as sixteen lowercase hex digits.
///
/// Tests compare this with the recorded `rust_hash`. It needs no dependency and
/// is stable across platforms because it only reads the decoded bytes.
pub fn canvas_hash(rgba: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in rgba {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Replace the `rust_hash=` field of every frame line in a manifest.
pub fn bless(manifest: &Path, hashes: &[String]) {
    let text = std::fs::read_to_string(manifest).expect("read manifest");
    let mut out = String::with_capacity(text.len() + hashes.len() * 20);
    let mut frame = 0;
    for line in text.lines() {
        if line.starts_with("frame ") {
            let mut line = line.to_string();
            if let Some(at) = line.find(" rust_hash=") {
                line.truncate(at);
            }
            line.push_str(" rust_hash=");
            line.push_str(&hashes[frame]);
            frame += 1;
            out.push_str(&line);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert_eq!(frame, hashes.len(), "manifest frame count");
    std::fs::write(manifest, out).expect("write manifest");
}
