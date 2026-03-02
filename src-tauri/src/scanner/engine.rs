//! Scan engine: orchestrates Phase 1 → Phase 2 → Phase 3.
//!
//! Provides a streaming API (`scan_home_excludes_streaming`) for live UI updates.

use rayon::prelude::*;
use std::path::PathBuf;

use super::artifacts;
use super::discovery;
use super::patterns::KNOWN_PATTERNS;
use super::types::*;
use super::walker;
use crate::util::format_size;

/// Streaming version: emits each result + phase progress via callbacks.
/// This is what the Tauri command uses — results arrive in the UI live.
pub fn scan_home_excludes_streaming(
    username: &str,
    on_found: impl Fn(&ScannedExclude) + Sync + Send,
    on_phase: impl Fn(&ScanPhase) + Sync + Send,
    on_progress: impl Fn(&std::path::Path) + Sync + Send,
) {
    scan_home_excludes_core(username, on_found, on_phase, on_progress);
}

/// Core scanner — emits results via `on_found` and phase markers via `on_phase`.
fn scan_home_excludes_core(
    username: &str,
    on_found: impl Fn(&ScannedExclude) + Sync + Send,
    on_phase: impl Fn(&ScanPhase) + Sync + Send,
    on_progress: impl Fn(&std::path::Path) + Sync + Send,
) {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(format!("/home/{}", username)));
    if !home.exists() {
        return;
    }

    let mut emitted_paths: Vec<String> = Vec::new();

    // ══════════════════════════════════════════════════════════
    //  Phase 1: Known-pattern scan
    // ══════════════════════════════════════════════════════════
    on_phase(&ScanPhase {
        phase: 1,
        label: "Bekannte Muster prüfen".into(),
    });

    let mut phase1_items: Vec<ScannedExclude> = Vec::new();

    for pat in KNOWN_PATTERNS {
        if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        on_progress(std::path::Path::new(pat.check_path));
        let exists = if pat.check_path.contains('*') {
            walker::glob_exists(&home, pat.check_path)
        } else {
            home.join(pat.check_path).exists()
        };

        if exists {
            let effective_paths: Vec<&str> = if pat.exclude_paths.is_empty() {
                vec![pat.check_path]
            } else {
                pat.exclude_paths.to_vec()
            };

            for ep in &effective_paths {
                phase1_items.push(ScannedExclude {
                    path: (*ep).to_string(),
                    category: pat.category.clone(),
                    reason: pat.reason.to_string(),
                    size_bytes: 0,
                    size_human: String::new(),
                    auto_exclude: true,
                });
            }
        }
    }

    // Parallel size computation for Phase 1 results
    {
        let indexed: Vec<(usize, PathBuf)> = phase1_items
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.path.contains('*'))
            .map(|(i, r)| (i, home.join(&r.path)))
            .collect();

        let (tx, rx) = std::sync::mpsc::channel();
        let on_progress_ref = &on_progress;
        rayon::scope(|s| {
            s.spawn(|_| {
                indexed
                    .into_par_iter()
                    .for_each_with(tx, |tx, (idx, path)| {
                        if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                            return;
                        }
                        let size = walker::dir_size(&path, Some(on_progress_ref));
                        let _ = tx.send((idx, size));
                    });
            });

            for (i, size) in rx {
                if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                phase1_items[i].size_bytes = size;
                phase1_items[i].size_human = format_size(size);
                on_found(&phase1_items[i]);
                emitted_paths.push(phase1_items[i].path.clone());
            }
        });

        if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }

        // Emit glob patterns (no size possible)
        for r in &phase1_items {
            if r.path.contains('*') {
                on_found(r);
                emitted_paths.push(r.path.clone());
            }
        }
    }

    // ══════════════════════════════════════════════════════════
    //  Phase 2: Deep discovery — find large dirs we missed
    // ══════════════════════════════════════════════════════════
    on_phase(&ScanPhase {
        phase: 2,
        label: "Verzeichnisse analysieren".into(),
    });

    let already_covered: Vec<String> = emitted_paths.clone();
    let scan_roots = ["", ".local/share", ".config", ".var/app"];
    let mut phase2_candidates: Vec<(String, PathBuf)> = Vec::new();

    for scan_root in &scan_roots {
        let scan_dir = if scan_root.is_empty() {
            home.clone()
        } else {
            home.join(scan_root)
        };
        if !scan_dir.exists() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(&scan_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();

                let rel_path = if scan_root.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{}", scan_root, name)
                };

                // Skip standard user directories and hidden files in home root
                if scan_root.is_empty() {
                    let lower_name = name.to_lowercase();
                    if lower_name == "desktop"
                        || lower_name == "documents"
                        || lower_name == "downloads"
                        || lower_name == "music"
                        || lower_name == "pictures"
                        || lower_name == "public"
                        || lower_name == "templates"
                        || lower_name == "videos"
                        || lower_name == "games"
                        || name.starts_with('.')
                    {
                        continue;
                    }
                }

                // Skip already covered
                if already_covered
                    .iter()
                    .any(|c| rel_path.starts_with(c) || c.starts_with(&rel_path))
                {
                    continue;
                }

                // Skip symlinks
                if path
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
                {
                    continue;
                }

                phase2_candidates.push((rel_path, path));
            }
        }
    }

    // Parallel size + classification
    let indexed: Vec<(usize, PathBuf)> = phase2_candidates
        .iter()
        .enumerate()
        .map(|(i, (_, abs))| (i, abs.clone()))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();
    let on_progress_ref = &on_progress;
    rayon::scope(|s| {
        s.spawn(|_| {
            indexed
                .into_par_iter()
                .for_each_with(tx, |tx, (idx, path)| {
                    if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    on_progress_ref(&path);
                    let size = walker::dir_size(&path, Some(on_progress_ref));
                    let _ = tx.send((idx, size));
                });
        });

        for (i, size) in rx {
            if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if size < SCAN_MIN_SIZE {
                continue;
            }
            let (rel_path, abs_path) = &phase2_candidates[i];
            if let Some(exc) = discovery::classify_unknown_dir(rel_path, abs_path, size) {
                emitted_paths.push(exc.path.clone());
                on_found(&exc);
            }
        }
    });

    if crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    // ══════════════════════════════════════════════════════════
    //  Phase 3: Recursive build artifact scan
    // ══════════════════════════════════════════════════════════
    on_phase(&ScanPhase {
        phase: 3,
        label: "Build-Artefakte suchen".into(),
    });

    // Snapshot the paths covered by Phase 1+2 so the artifact scanner
    // can skip anything nested inside them (generic — no hardcoded IDE paths).
    let covered: Vec<String> = emitted_paths.clone();

    artifacts::scan_project_artifacts(
        &home,
        &covered,
        |art| {
            // Deduplicate: also check against paths emitted during Phase 3 itself
            let is_child = emitted_paths.iter().any(|p| {
                art.path.starts_with(p) && art.path.as_bytes().get(p.len()) == Some(&b'/')
            });
            if !is_child && !emitted_paths.contains(&art.path) {
                emitted_paths.push(art.path.clone());
                on_found(&art);
            }
        },
        &on_progress,
    );
}
