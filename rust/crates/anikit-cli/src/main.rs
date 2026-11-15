//! Command line entry point: argument parsing, route dispatch and messages.

mod cli;
mod png;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("FAIL {err}");
            ExitCode::from(1)
        }
    }
}
