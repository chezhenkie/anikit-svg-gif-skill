//! Repository hygiene: ASCII text, LF endings, no machine paths, resolved links.
//!
//! Part 08 lists these checks as part of the Rust gate once `tools/check.py`
//! is gone. The font licence under `anikit-render-text/assets` is third-party
//! text and is left out of the ASCII scan.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if name == ".git" || name == "target" || name == "scratch" {
            continue;
        }
        if path.is_dir() {
            collect(&path, files);
        } else if is_text(&name) && !path.components().any(|part| part.as_os_str() == "assets") {
            files.push(path);
        }
    }
}

fn is_text(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    const EXACT: [&str; 4] = [".gitattributes", ".gitignore", "license", "cargo.lock"];
    const EXT: [&str; 12] = [
        ".md", ".rs", ".toml", ".yml", ".yaml", ".txt", ".json", ".py", ".cjs", ".html", ".css",
        ".svg",
    ];
    EXACT.contains(&name.as_str()) || EXT.iter().any(|ext| name.ends_with(ext))
}

fn text_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&root(), &mut files);
    files.sort();
    assert!(
        files.len() > 40,
        "the walk found only {} text files",
        files.len()
    );
    files
}

fn relative(path: &Path) -> String {
    path.strip_prefix(root())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[test]
fn text_files_are_ascii_with_lf_endings() {
    let mut problems = Vec::new();
    for path in text_files() {
        let bytes = std::fs::read(&path).expect("read text file");
        if bytes.contains(&b'\r') {
            problems.push(format!("{}: carriage return", relative(&path)));
        }
        if let Some(at) = bytes.iter().position(|byte| !byte.is_ascii()) {
            problems.push(format!("{}: non-ASCII byte at {at}", relative(&path)));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn no_machine_paths_are_tracked() {
    // Rust sources hold paths as test data, so this scan covers the prose and
    // the scripts; ASCII and LF still cover every text file.
    let mut problems = Vec::new();
    for path in text_files() {
        if path.extension().is_some_and(|ext| ext == "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read text file");
        for line in text.lines() {
            let lowered = line.to_ascii_lowercase();
            let drive = lowered
                .as_bytes()
                .windows(3)
                .any(|w| w[1] == b':' && w[2] == b'\\' && w[0].is_ascii_alphabetic());
            if drive || line.contains("/home/") || line.contains("/Users/") {
                problems.push(format!("{}: {line}", relative(&path)));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn no_credentials_are_tracked() {
    const PREFIXES: [&str; 5] = ["ghp_", "gho_", "sk-", "AKIA", "PRIVATE KEY"];
    let mut problems = Vec::new();
    for path in text_files() {
        if path
            .file_name()
            .is_some_and(|name| name == "repo_hygiene.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read text file");
        for (number, line) in text.lines().enumerate() {
            if PREFIXES.iter().any(|prefix| line.contains(prefix)) {
                problems.push(format!(
                    "{}:{}: possible credential",
                    relative(&path),
                    number + 1
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn markdown_links_resolve() {
    let mut problems = Vec::new();
    for path in text_files() {
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read markdown");
        let base = path.parent().expect("markdown has a parent");
        for target in link_targets(&text) {
            if target.starts_with('#') || target.contains("://") {
                continue;
            }
            let target = target.split('#').next().unwrap_or_default();
            if target.is_empty() {
                continue;
            }
            if !base.join(target).exists() {
                problems.push(format!("{}: missing {target}", relative(&path)));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn link_targets(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else {
            break;
        };
        targets.push(rest[..end].trim());
        rest = &rest[end + 1..];
    }
    targets
}

#[test]
fn the_third_party_notices_match_the_lock_file() {
    let notices =
        std::fs::read_to_string(root().join("THIRD-PARTY-NOTICES.txt")).expect("notices file");
    let lock = std::fs::read_to_string(root().join("rust/Cargo.lock")).expect("Cargo.lock");
    let mut resolved = std::collections::BTreeSet::new();
    for line in lock.lines() {
        if let Some(rest) = line.strip_prefix("name = \"") {
            let name = rest.trim_end_matches('"');
            if !name.starts_with("anikit-") {
                resolved.insert(name.to_string());
            }
        }
    }
    let mut listed = std::collections::BTreeSet::new();
    let mut inside = false;
    for line in notices.lines() {
        if line == "Crates:" {
            inside = true;
            continue;
        }
        if line == "(end)" {
            break;
        }
        if inside {
            if let Some(name) = line.trim_start().split_whitespace().next() {
                listed.insert(name.to_string());
            }
        }
    }
    let missing: Vec<&String> = resolved.difference(&listed).collect();
    assert!(missing.is_empty(), "missing from the notices: {missing:?}");
    let stale: Vec<&String> = listed.difference(&resolved).collect();
    assert!(stale.is_empty(), "listed but not resolved: {stale:?}");
    assert!(notices.contains("Apache License"));
    assert!(notices.contains("MIT License"));
}

#[test]
fn the_skill_header_names_the_skill_and_a_version() {
    let text = std::fs::read_to_string(root().join("agent-skill/SKILL.md")).expect("SKILL.md");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("---"), "SKILL.md has no frontmatter");
    let mut header = Vec::new();
    for line in lines {
        if line == "---" {
            break;
        }
        header.push(line);
    }
    let field = |name: &str| {
        header
            .iter()
            .find_map(|line| {
                line.trim_start()
                    .strip_prefix(name)?
                    .trim()
                    .strip_prefix(':')
            })
            .map(|value| value.trim().trim_matches('"').to_string())
    };
    assert_eq!(field("name").as_deref(), Some("animated-gif"));
    assert_eq!(field("license").as_deref(), Some("MIT"));
    let version = field("version").expect("metadata version");
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3, "version {version} is not X.Y.Z");
    assert!(
        parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "version {version} is not numeric"
    );
}
