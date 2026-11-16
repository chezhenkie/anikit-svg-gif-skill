//! Finding a Chromium based browser and starting it with a debugging port.
//!
//! The browser is started once per run against an empty profile, and killed
//! with its profile removed when the run ends or fails.

use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anikit_core::{Error, Result};
use serde_json::Value;

/// The environment variable that names a browser explicitly.
pub const BROWSER_ENV: &str = "GIFKIT_BROWSER";

const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(20);

/// Names looked up on PATH, in order.
const PATH_NAMES: [&str; 6] = [
    "ungoogled-chromium",
    "chromium",
    "chromium-browser",
    "google-chrome",
    "google-chrome-stable",
    "msedge",
];

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A random value for this process, drawn from a hasher's random keys.
fn process_seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(std::process::id() as u64);
        hasher.finish()
    })
}

/// A unique path under the system temp directory.
///
/// The token mixes the per-process seed with the pid and a counter, so a
/// pre-created path cannot be guessed from outside the process.
fn unique(name: &str) -> PathBuf {
    let count = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(process_seed());
    hasher.write_u64(std::process::id() as u64);
    hasher.write_u64(count);
    std::env::temp_dir().join(format!(
        "anikit-{name}-{}-{:016x}",
        std::process::id(),
        hasher.finish()
    ))
}

/// Draw a fresh path and create it, drawing again when the name is taken.
///
/// A taken name is a stale file or another process, so the run takes a new
/// name instead of writing through the existing path.
fn take<T>(
    name: &str,
    mut create: impl FnMut(&Path) -> std::io::Result<T>,
) -> Result<(PathBuf, T)> {
    for _ in 0..8 {
        let path = unique(name);
        match create(&path) {
            Ok(value) => return Ok((path, value)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(Error::io(format!(
                    "cannot create {}: {err}",
                    path.display()
                )));
            }
        }
    }
    Err(Error::io(format!(
        "cannot find a free temporary name for {name}"
    )))
}

/// A new directory under the system temp directory.
pub(crate) fn unique_dir(name: &str) -> Result<PathBuf> {
    take(name, |path| std::fs::create_dir(path)).map(|(path, ())| path)
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
    if let Some(found) = on_path(&std::env::var_os("PATH").unwrap_or_default()) {
        return Ok(found);
    }
    Err(Error::invalid(format!(
        "no Chromium based browser found; pass --browser PATH or set {BROWSER_ENV}"
    )))
}

/// The first named browser that exists in a PATH value.
fn on_path(path: &std::ffi::OsStr) -> Option<PathBuf> {
    for name in PATH_NAMES {
        for dir in std::env::split_paths(path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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

/// Why one launch attempt produced no browser.
enum Launch {
    /// The process ended before it opened its port. A kernel that refuses the
    /// sandbox reports itself this way.
    Exited(Error),
    /// A spawn error, a timeout or an unreadable child.
    Failed(Error),
}

/// The flags for one attempt, other than the profile path.
///
/// The page never gets `--allow-file-access-from-files`: a file page could
/// otherwise read any local file its script names.
fn arguments(sandbox: bool) -> Vec<&'static str> {
    let mut args = vec![
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--force-color-profile=srgb",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--mute-audio",
        "--remote-debugging-port=0",
    ];
    if !sandbox {
        args.push("--no-sandbox");
    }
    args
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
    /// The first attempt keeps Chromium's sandbox. A kernel that restricts
    /// unprivileged user namespaces makes the browser exit before it opens the
    /// port, and only then is the attempt repeated without the sandbox. Any
    /// other failure is a race for resources, which a second attempt with a
    /// fresh profile covers, as before.
    pub fn launch(exe: &Path) -> Result<Browser> {
        match Self::start(exe, true) {
            Ok(browser) => Ok(browser),
            Err(Launch::Exited(first)) => Self::start(exe, false).map_err(|_| first),
            Err(Launch::Failed(first)) => {
                std::thread::sleep(Duration::from_millis(500));
                Self::start(exe, true).map_err(|_| first)
            }
        }
    }

    fn start(exe: &Path, sandbox: bool) -> std::result::Result<Browser, Launch> {
        let profile = unique_dir("profile").map_err(Launch::Failed)?;
        let (log, stderr) = match take("browser.log", |path| File::create_new(path)) {
            Ok(pair) => pair,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(Launch::Failed(err));
            }
        };
        let child = Command::new(exe)
            .args(arguments(sandbox))
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|err| {
                Launch::Failed(Error::io(format!("cannot start {}: {err}", exe.display())))
            })?;

        let mut browser = Browser {
            child,
            profile,
            log,
            port: 0,
        };
        browser.port = browser.wait_for_port()?;
        Ok(browser)
    }

    fn wait_for_port(&mut self) -> std::result::Result<u16, Launch> {
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
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    return Err(Launch::Exited(Error::io(format!(
                        "browser exited with {status} before opening a debugging port{}",
                        log_note(&self.log)
                    ))));
                }
                Ok(None) => {}
                Err(err) => return Err(Launch::Failed(err.into())),
            }
            if Instant::now() > deadline {
                return Err(Launch::Failed(Error::io(
                    "browser did not open a debugging port within 20 s",
                )));
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
    fn msedge_on_path_is_found_and_chromium_still_wins() {
        let dir = std::env::temp_dir().join(format!("anikit-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("msedge"), b"").unwrap();
        let path = dir.clone().into_os_string();
        assert_eq!(on_path(&path), Some(dir.join("msedge")));
        std::fs::write(dir.join("chromium"), b"").unwrap();
        assert_eq!(on_path(&path), Some(dir.join("chromium")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn temp_names_are_unique() {
        let first = unique("profile");
        let second = unique("profile");
        assert_ne!(first, second);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with(&format!("anikit-profile-{}-", std::process::id())),
            "{name}"
        );
        // A random tail keeps a path from being guessed before the run starts.
        let tail = name.rsplit('-').next().unwrap();
        assert_eq!(tail.len(), 16, "{name}");
        assert!(tail.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
    }

    #[test]
    fn a_taken_name_is_skipped() {
        let mut attempts = 0;
        let (path, ()) = take("probe", |path| {
            attempts += 1;
            std::fs::write(path, b"stale")?;
            if attempts < 3 {
                return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(attempts, 3, "the first two names were taken");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "stale");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn another_failure_is_reported_with_its_path() {
        let err = take("probe", |_| -> std::io::Result<()> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        assert!(err.to_string().contains("cannot create"), "{err}");
        assert!(err.to_string().contains("anikit-probe-"), "{err}");
    }

    #[test]
    fn the_sandbox_is_dropped_only_on_the_fallback() {
        let guarded = arguments(true);
        assert!(!guarded.contains(&"--no-sandbox"));
        assert!(arguments(false).contains(&"--no-sandbox"));
        for args in [guarded, arguments(false)] {
            assert!(!args.contains(&"--allow-file-access-from-files"));
        }
    }
}
