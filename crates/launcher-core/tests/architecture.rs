//! Architecture guard: enforces the dependency rule inside `launcher-core`.
//!
//! - `domain` must not import `application` or `infrastructure`.
//! - `application` must not import `infrastructure`.
//! - The crate must never depend on `tauri`.

// Test-only code: panicking on an unreadable source file is the intended failure.
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

const CRATE_ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Returns the forbidden import paths found in `source` for the given layer.
fn violations(layer: &str, source: &str) -> Vec<String> {
    let forbidden: &[&str] = match layer {
        "domain" => &["application", "infrastructure"],
        "application" => &["infrastructure"],
        _ => &[],
    };
    let code = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut found = Vec::new();
    for module in forbidden {
        let qualified = format!("crate::{module}");
        if code.contains(&qualified) {
            found.push(qualified);
        } else if use_statements(&code).any(|stmt| path_segments(stmt).any(|seg| seg == *module)) {
            found.push((*module).to_owned());
        }
    }
    let imports_tauri =
        use_statements(&code).any(|stmt| path_segments(stmt).next() == Some("tauri"));
    if imports_tauri || code.contains("tauri::") {
        found.push("tauri".to_owned());
    }
    found
}

/// Yields the path part of every `use` statement (`pub use` included).
fn use_statements(code: &str) -> impl Iterator<Item = &str> {
    code.split(';').filter_map(|statement| {
        statement.lines().find_map(|line| {
            let line = line.trim_start();
            let line = line
                .strip_prefix("pub(crate) ")
                .or_else(|| line.strip_prefix("pub "))
                .unwrap_or(line);
            line.strip_prefix("use ")
        })
    })
}

fn path_segments(statement: &str) -> impl Iterator<Item = &str> {
    statement
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|segment| !segment.is_empty())
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

fn assert_layer_is_clean(layer: &str) {
    let dir = Path::new(CRATE_ROOT).join("src").join(layer);
    let mut found = Vec::new();
    for file in rust_files(&dir) {
        let source = fs::read_to_string(&file).expect("source file must be readable");
        for violation in violations(layer, &source) {
            found.push(format!("{}: {violation}", file.display()));
        }
    }
    assert!(
        found.is_empty(),
        "dependency rule violated:\n{}",
        found.join("\n")
    );
}

#[test]
fn domain_does_not_depend_on_outer_layers() {
    assert_layer_is_clean("domain");
}

#[test]
fn application_does_not_depend_on_infrastructure() {
    assert_layer_is_clean("application");
}

#[test]
fn core_crate_does_not_depend_on_tauri() {
    let manifest = fs::read_to_string(Path::new(CRATE_ROOT).join("Cargo.toml"))
        .expect("Cargo.toml must be readable");
    let offending: Vec<&str> = manifest
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("tauri") || line.contains("\"tauri"))
        .collect();
    assert!(
        offending.is_empty(),
        "launcher-core must not depend on tauri: {offending:?}"
    );
}

#[test]
fn scanner_rejects_forbidden_imports() {
    assert_eq!(
        violations("domain", "use crate::infrastructure::http::Client;"),
        vec!["crate::infrastructure".to_owned()]
    );
    assert_eq!(
        violations("domain", "use crate::application::launch_instance;"),
        vec!["crate::application".to_owned()]
    );
    assert_eq!(
        violations(
            "application",
            "use crate::{domain::Instance, infrastructure::fs};"
        ),
        vec!["infrastructure".to_owned()]
    );
    assert_eq!(
        violations("domain", "use tauri::AppHandle;"),
        vec!["tauri".to_owned()]
    );
}

#[test]
fn scanner_accepts_allowed_imports() {
    assert!(violations("application", "use crate::domain::ports::Clock;").is_empty());
    assert!(violations("domain", "use super::ports::Clock;").is_empty());
}
