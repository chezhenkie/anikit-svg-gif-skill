//! Command line entry point: argument parsing, route dispatch and messages.
//!
//! The routes are not implemented yet. This stub exists so the workspace has
//! one binary under the final name and the crate graph is checked by cargo.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("FAIL anikit: routes are not implemented yet");
    ExitCode::from(1)
}
