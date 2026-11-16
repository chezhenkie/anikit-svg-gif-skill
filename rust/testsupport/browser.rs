//! The browser a test drives, and the one case where a test may skip.
//!
//! A missing browser fails the test, so a host without one cannot look green.
//! `ANIKIT_SKIP_BROWSER_TESTS=1` allows the skip on purpose and says so in the
//! log.

use std::path::PathBuf;

use anikit_render_svg::discover;

/// The browser for a test, or None when a skip was asked for.
pub fn browser() -> Option<PathBuf> {
    match discover(None) {
        Ok(path) => Some(path),
        Err(error) => {
            if std::env::var("ANIKIT_SKIP_BROWSER_TESTS").as_deref() == Ok("1") {
                eprintln!("skipping browser test: {error}");
                return None;
            }
            panic!("no browser for this test: {error}; set ANIKIT_SKIP_BROWSER_TESTS=1 to skip it");
        }
    }
}
