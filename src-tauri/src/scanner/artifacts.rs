//! Phase 3: Build artifact scanner.
//!
//! Finds build artifacts (node_modules, target/, .venv, etc.) inside
//! project directories. Uses native parallel FS walking instead of `find`.

use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::types::*;
use super::walker;
use crate::util::format_size;

/// Scan for build artifacts inside project directories.
///
/// Finds well-known artifact directories (node_modules, target/debug, .venv, etc.)
/// up to 5 levels deep, computes their sizes in parallel, and returns results
/// for anything ≥ 10 MB.
///
/// `covered_paths` — relative paths already emitted by Phase 1/2. Any artifact
/// found inside one of these is skipped (generic — covers IDE extensions,
/// runtime directories, toolchains, etc. without hardcoding).
pub fn scan_project_artifacts(
    home: &Path,
    covered_paths: &[String],
    mut on_found: impl FnMut(ScannedExclude) + Send,
    on_progress: &(impl Fn(&std::path::Path) + Sync + Send),
) {
    // Build artifact dir names → (reason, required_parent)
    let artifact_info: HashMap<&str, (&str, Option<&str>)> = [
        (
            "node_modules",
            (
                "Node.js Abhängigkeiten (npm/pnpm/yarn install regeneriert)",
                None,
            ),
        ),
        (
            ".venv",
            (
                "Python Virtual Environment (python -m venv regeneriert)",
                None,
            ),
        ),
        ("venv", ("Python Virtual Environment", None)),
        ("__pycache__", ("Python Bytecode Cache", None)),
        (
            "debug",
            (
                "Rust Debug Build-Artefakte (cargo build regeneriert)",
                Some("target"),
            ),
        ),
        ("release", ("Rust Release Build-Artefakte", Some("target"))),
        (".next", ("Next.js Build-Output", None)),
        ("dist", ("Build Output", None)),
        (".gradle", ("Gradle Projekt-Cache", None)),
        (".dart_tool", ("Dart/Flutter Tool Cache", None)),
        (".pub-cache", ("Dart pub Package Cache", None)),
        ("intermediates", ("Android Build-Artefakte", Some("build"))),
    ]
    .into_iter()
    .collect();

    let names: Vec<&str> = artifact_info.keys().copied().collect();

    on_progress(std::path::Path::new(
        "Suche nach Projekt-Ordnern (node_modules, target, etc.)...",
    ));

    // Native parallel find — replaces `find $home -maxdepth 5 -type d \( -name … \)`
    let found = walker::find_dirs_by_name(home, &names, 5, Some(on_progress));

    // Filter and build candidates
    let mut candidates: Vec<(PathBuf, String, &str)> = Vec::new();
    for abs in found {
        if !abs.exists() {
            continue;
        }

        let dir_name = abs
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let (reason, required_parent) = match artifact_info.get(dir_name.as_str()) {
            Some(info) => *info,
            None => continue,
        };

        // For patterns like "target/debug", verify parent matches
        if let Some(parent_name) = required_parent {
            let actual_parent = abs
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if actual_parent != parent_name {
                continue;
            }
        }

        let rel = abs
            .strip_prefix(home)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if rel.is_empty() {
            continue;
        }

        let search_from = abs.parent().unwrap_or(abs.as_path());
        let project_root = find_project_root(search_from, home, 6);

        if requires_project_context(&dir_name) && project_root.is_none() {
            continue;
        }

        // Generic runtime-extension guard:
        // node_modules inside hidden ".../extensions/..." stores are usually
        // app runtime internals, not user projects.
        if dir_name == "node_modules"
            && is_runtime_extension_node_modules(&abs, home, project_root.as_deref())
        {
            continue;
        }

        // Skip artifacts that live inside paths already covered by Phase 1/2.
        // This is generic: covers IDE extensions (.vscode-server, .vscode/extensions),
        // runtime dirs (.local/share/flatpak), toolchains (.rustup), etc.
        // No hardcoded paths needed — if Phase 1 already flagged the parent,
        // we don't re-flag its internal build artifacts.
        let inside_covered = covered_paths.iter().any(|covered| {
            rel.starts_with(covered) && rel.as_bytes().get(covered.len()) == Some(&b'/')
        });
        if inside_covered {
            continue;
        }

        // Skip nested artifacts of the same type (e.g. node_modules inside node_modules,
        // dist inside node_modules) — the parent artifact already covers them.
        {
            let parts: Vec<&str> = rel.split('/').collect();
            // Check if any ancestor directory is the same artifact type
            let is_nested_same = parts
                .iter()
                .rev()
                .skip(1) // skip the artifact dir itself
                .any(|p| names.contains(p));
            if is_nested_same {
                continue;
            }
        }

        // For target/debug → display as target/debug; for intermediates → build/intermediates
        let display_rel = if required_parent.is_some() {
            if let Some(parent_path) = abs.parent() {
                parent_path
                    .strip_prefix(home)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or(rel.clone())
            } else {
                rel.clone()
            }
        } else {
            rel.clone()
        };

        candidates.push((abs, display_rel, reason));
    }

    // Parallel size computation using rayon (via walker)
    let indexed: Vec<(usize, PathBuf)> = candidates
        .iter()
        .enumerate()
        .map(|(i, (abs, _, _))| (i, abs.clone()))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();
    rayon::scope(|s| {
        s.spawn(|_| {
            indexed
                .into_par_iter()
                .for_each_with(tx, |tx, (idx, path)| {
                    if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    on_progress(&path);
                    let size = walker::dir_size(&path, Some(on_progress));
                    let _ = tx.send((idx, size));
                });
        });

        for (i, size) in rx {
            if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if size < 10 * 1024 * 1024 {
                continue; // Skip tiny ones (< 10 MB)
            }
            let (_abs, rel, reason) = &candidates[i];
            on_found(ScannedExclude {
                path: rel.clone(),
                category: ExcludeCategory::BuildArtifact,
                reason: format!("{} ({})", reason, format_size(size)),
                size_bytes: size,
                size_human: format_size(size),
                auto_exclude: true,
            });
        }
    });
}

fn has_project_marker(dir: &Path) -> bool {
    let markers = [
        ".git",
        "package.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "package-lock.json",
        "Cargo.toml",
        "go.mod",
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "build.gradle",
        "build.gradle.kts",
        "pom.xml",
        "Makefile",
        "CMakeLists.txt",
    ];
    markers.iter().any(|m| dir.join(m).exists())
}

fn looks_like_project_root(dir: &Path) -> bool {
    if dir.join(".git").exists() {
        return true;
    }

    if !has_project_marker(dir) {
        return false;
    }

    // If a package manager lockfile or source directory exists, this is very
    // likely a user/dev project rather than runtime extension payload.
    let lockfile = dir.join("pnpm-lock.yaml").exists()
        || dir.join("package-lock.json").exists()
        || dir.join("yarn.lock").exists()
        || dir.join("poetry.lock").exists()
        || dir.join("Pipfile.lock").exists();
    let src_dir = dir.join("src").is_dir() || dir.join("app").is_dir();

    lockfile || src_dir
}

fn requires_project_context(dir_name: &str) -> bool {
    matches!(
        dir_name,
        "node_modules"
            | ".next"
            | "dist"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".dart_tool"
            | "intermediates"
            | "target"
            | "debug"
            | "release"
    )
}

fn find_project_root(start: &Path, home: &Path, max_up: usize) -> Option<PathBuf> {
    let mut cur = Some(start);
    let mut steps = 0usize;

    while let Some(dir) = cur {
        if dir.starts_with(home) && looks_like_project_root(dir) {
            return Some(dir.to_path_buf());
        }

        if dir == home || steps >= max_up {
            break;
        }

        cur = dir.parent();
        steps += 1;
    }

    None
}

fn is_runtime_extension_node_modules(abs: &Path, home: &Path, project_root: Option<&Path>) -> bool {
    if let Some(root) = project_root {
        if root.join(".git").exists() {
            return false;
        }
    }

    let Ok(rel) = abs.strip_prefix(home) else { return false };

    let segments: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    let Some(ext_idx) = segments.iter().position(|s| s == "extensions") else { return false };

    let Some(nm_idx) = segments.iter().rposition(|s| s == "node_modules") else { return false };

    if ext_idx >= nm_idx {
        return false;
    }

    // Only treat as runtime extension store when hidden/system segments exist
    // before "extensions" (e.g. .vscode-server, .local/share/...).
    let has_hidden_before_extensions = segments[..ext_idx].iter().any(|s| s.starts_with('.'));
    if !has_hidden_before_extensions {
        return false;
    }

    // If we have a strong project root signal, keep it.
    if let Some(root) = project_root {
        if looks_like_project_root(root) {
            return false;
        }
    }

    true
}
