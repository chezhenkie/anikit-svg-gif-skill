//! Finding a Chromium based browser and starting it with a debugging port.
//!
//! The browser is started once per run against an empty profile, and killed
//! with its profile removed when the run ends or fails.

use std::fs::File;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anikit_core::{Error, Result};
use serde_json::Value;

/// The environment variable that names a browser explicitly.
pub const BROWSER_ENV: &str = "GIFKIT_BROWSER";

const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(20);

/// Names looked up on PATH, in order.
const PATH_NAMES: [&str; 5] = [
    "ungoogled-chromium",
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
];

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A unique path under the system temp directory.
pub(crate) fn unique(name: &str) -> PathBuf {
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("anikit-{name}-{}-{count}", std::process::id()))
}

/// The browser to drive: the explicit path, the environment override, or the
/// first installed one.
pub fn discover(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if !path.is_file() {
            return Err(Error::invalid(format!(
                "browser not found: {}",
                path.display()
            )));
        }
        return Ok(path.to_path_buf());
    }
    if let Some(value) = std::env::var_os(BROWSER_ENV) {
        let path = PathBuf::from(value);
        if !path.is_file() {
            return Err(Error::invalid(format!(
                "{BROWSER_ENV} does not exist: {}",
                path.display()
            )));
        }
        return Ok(path);
    }
    for candidate in installed() {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    for name in PATH_NAMES {
        for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(Error::invalid(format!(
        "no Chromium based browser found; pass --browser PATH or set {BROWSER_ENV}"
    )))
}

/// Browser locations by platform, most likely first.
fn installed() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    if cfg!(target_os = "windows") {
        let local = var("LOCALAPPDATA");
        let program = var("PROGRAMFILES");
        let program_x86 = var("ProgramFiles(x86)");
        let mut push = |root: Option<PathBuf>, tail: &str| {
            if let Some(root) = root {
                found.push(root.join(tail));
            }
        };
        push(local.clone(), "Chromium/Application/chrome.exe");
        push(local, "Google/Chrome/Application/chrome.exe");
        push(program.clone(), "Google/Chrome/Application/chrome.exe");
        push(program_x86.clone(), "Google/Chrome/Application/chrome.exe");
        push(program_x86, "Microsoft/Edge/Application/msedge.exe");
        push(program, "Microsoft/Edge/Application/msedge.exe");
    } else if cfg!(target_os = "macos") {
        for path in [
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ] {
            found.push(PathBuf::from(path));
        }
    }
    found
}

/// A running browser with an open debugging port.
pub struct Browser {
    child: Child,
    profile: PathBuf,
    log: PathBuf,
    port: u16,
}

impl Browser {
    /// Start the browser headless and wait for its debugging port.
    ///
    /// A launch can lose the race for resources when several browsers start at
    /// once; a second attempt with a fresh profile covers that. A browser that
    /// cannot run at all fails both times.
    pub fn launch(exe: &Path) -> Result<Browser> {
        match Self::start(exe) {
            Ok(browser) => Ok(browser),
            Err(first) => {
                std::thread::sleep(Duration::from_millis(500));
                Self::start(exe).map_err(|_| first)
            }
        }
    }

    fn start(exe: &Path) -> Result<Browser> {
        let profile = unique("profile");
        let log = unique("browser.log");
        let _ = std::fs::remove_dir_all(&profile);
        let stderr = File::create(&log)
            .map_err(|err| Error::io(format!("cannot write {}: {err}", log.display())))?;
        let child = Command::new(exe)
            .args([
                "--headless=new",
                "--disable-gpu",
                "--hide-scrollbars",
                "--force-color-profile=srgb",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--mute-audio",
                "--allow-file-access-from-files",
                // The Python route ran under Playwright, which disables the
                // Chromium sandbox by default. CI kernels also restrict
                // unprivileged user namespaces, and the browser aborts before
                // it opens a debugging port when the sandbox cannot start.
                "--no-sandbox",
                "--remote-debugging-port=0",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|err| Error::io(format!("cannot start {}: {err}", exe.display())))?;

        let mut browser = Browser {
            child,
            profile,
            log,
            port: 0,
        };
        browser.port = browser.wait_for_port()?;
        Ok(browser)
    }

    fn wait_for_port(&mut self) -> Result<u16> {
        let active = self.profile.join("DevToolsActivePort");
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Ok(text) = std::fs::read_to_string(&active)
                && let Some(port) = text
                    .lines()
                    .next()
                    .and_then(|line| line.trim().parse().ok())
            {
                return Ok(port);
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(Error::io(format!(
                    "browser exited with {status} before opening a debugging port{}",
                    log_note(&self.log)
                )));
            }
            if Instant::now() > deadline {
                return Err(Error::io(
                    "browser did not open a debugging port within 20 s",
                ));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Wait for the first page target and return its WebSocket address.
    pub fn page_web_socket(&mut self) -> Result<String> {
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            if let Ok(body) = http_get(self.port, "/json/list")
                && let Ok(targets) = serde_json::from_str::<Value>(&body)
                && let Some(url) = page_target(&targets)
            {
                return Ok(url);
            }
            if let Some(status) = self.child.try_wait()? {
                return Err(Error::io(format!(
                    "browser exited with {status} before a page target appeared{}",
                    log_note(&self.log)
                )));
            }
            if Instant::now() > deadline {
                return Err(Error::io("browser did not open a page target within 20 s"));
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.profile).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = std::fs::remove_file(&self.log);
    }
}

/// The first page target's debugging address.
fn page_target(targets: &Value) -> Option<String> {
    targets.as_array()?.iter().find_map(|target| {
        let kind = target.get("type")?.as_str()?;
        if kind != "page" {
            return None;
        }
        target
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .map(|url| url.to_string())
    })
}

/// One small HTTP request to the browser's local debugging server.
///
/// The server keeps the socket open after the reply, so the body is read by its
/// declared length rather than until end of stream.
fn http_get(port: u16, path: &str) -> Result<String> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|err| Error::io(format!("cannot reach the browser: {err}")))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::io("browser closed the connection without a reply"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if status.is_empty() {
            status = line.trim().to_string();
        } else if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    if !status.contains(" 200") {
        return Err(Error::io(format!("browser answered {status}")));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// The last non-empty line of the browser log, for an error message.
fn log_note(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    match text.lines().rev().find(|line| !line.trim().is_empty()) {
        Some(line) => format!(": {}", line.trim()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_target_is_picked_from_the_list() {
        let targets: Value = serde_json::from_str(
            r#"[
                {"type": "background_page", "webSocketDebuggerUrl": "ws://x/b"},
                {"type": "page", "webSocketDebuggerUrl": "ws://x/page"}
            ]"#,
        )
        .unwrap();
        assert_eq!(page_target(&targets).as_deref(), Some("ws://x/page"));
        assert!(page_target(&Value::Null).is_none());
    }

    #[test]
    fn a_missing_browser_path_fails() {
        let err = discover(Some(Path::new("no-such-browser.exe"))).unwrap_err();
        assert!(err.to_string().contains("browser not found"));
    }

    #[test]
    fn temp_names_are_unique() {
        assert_ne!(unique("profile"), unique("profile"));
    }
}
