//! Cleanup commands: scan for deletable files and remove them.

use super::helpers::*;
use rayon::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::Emitter;

/// Number of items per batch before emitting to the frontend.
const CLEANUP_EMIT_BATCH_SIZE: usize = 10;
/// Throttle interval (ms) for progress event emission during scan.
const CLEANUP_PROGRESS_THROTTLE_MS: u128 = 50;
/// Size threshold (bytes) for sending build artifacts to AI review.
const AI_REVIEW_SIZE_THRESHOLD: u64 = 300 * 1024 * 1024;

/// Represents a deletable item shown in the UI.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct CleanupItem {
    pub path: String,
    pub abs_path: String,
    pub category: String,
    pub reason: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub safe: bool,
    pub ai_checked: bool,
    pub ai_confidence: Option<f64>,
    pub ai_note: Option<String>,
}

/// Result of a deletion operation.
#[derive(serde::Serialize, Clone, Debug)]
pub struct DeleteResult {
    pub path: String,
    pub success: bool,
    pub error: Option<String>,
}

#[tauri::command]
pub fn cancel_scan() {
    crate::scanner::CANCEL_SCAN.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Scan home directory for deletable junk files (caches, build artifacts, etc.)
/// Streams results via events for live UI updates, then returns the full list.
#[tauri::command]
pub async fn scan_cleanup(
    app: tauri::AppHandle,
    ai_assist: Option<bool>,
) -> Result<Vec<CleanupItem>, String> {
    let ai_assist = ai_assist.unwrap_or_default();
    crate::scanner::CANCEL_SCAN.store(false, std::sync::atomic::Ordering::SeqCst);
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(format!("/home/{}", username)));

    log_activity_with_app(
        &app,
        "cleanup",
        &format!(
            "Cleanup-Scan gestartet — suche löschbare Dateien...{}",
            if ai_assist {
                " (KI-Assistent aktiv)"
            } else {
                ""
            }
        ),
    );
    log_activity_with_app(
        &app,
        "cleanup",
        &format!("KI-Assistent: {}", if ai_assist { "AN" } else { "AUS" }),
    );

    let app_clone = app.clone();
    let items = tokio::task::spawn_blocking(move || {
        let results = std::sync::Mutex::new(Vec::<CleanupItem>::new());
        let batch = std::sync::Mutex::new(Vec::<CleanupItem>::new());
        let start_time = std::time::Instant::now();
        let phase_start = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
        let ai_checked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ai_downgraded = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ai_unavailable = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let ai_cache_hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        crate::scanner::scan_home_excludes_streaming(
            &username,
            |exclude| {
                let mut safe = is_safe_to_delete(&exclude.category, &exclude.path, &home);
                let mut reason = exclude.reason.clone();
                let mut ai_checked_item = false;
                let mut ai_confidence: Option<f64> = None;
                let mut ai_note: Option<String> = None;

                if ai_assist && safe && should_ai_review_item(&exclude.path, &exclude.category, exclude.size_bytes) {
                    match ai_review_item(&exclude.path, &exclude.category, &reason, &home) {
                        AiReviewOutcome::Reviewed(decision) => {
                            ai_checked.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            ai_checked_item = true;
                            ai_confidence = Some(decision.confidence);
                            if !decision.note.is_empty() {
                                ai_note = Some(decision.note.clone());
                            }
                            if decision.from_cache {
                                ai_cache_hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                            // confidence = how safe to delete. downgrade=true means KI thinks it's NOT safe.
                            // Downgrade when KI says unsafe AND confidence-of-safety is low (< 0.40).
                            if decision.downgrade && decision.confidence <= 0.40 {
                                safe = false;
                                reason = format!("{} · KI-Hinweis: {}", reason, decision.note);
                                ai_downgraded.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        AiReviewOutcome::NoJudgment => {}
                        AiReviewOutcome::Unavailable => {
                            ai_unavailable.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }

                let abs_path = home.join(&exclude.path).to_string_lossy().into_owned();
                let item = CleanupItem {
                    path: exclude.path.clone(),
                    abs_path,
                    category: format!("{:?}", exclude.category),
                    reason,
                    size_bytes: exclude.size_bytes,
                    size_human: exclude.size_human.clone(),
                    safe,
                    ai_checked: ai_checked_item,
                    ai_confidence,
                    ai_note,
                };

                if let Ok(mut b) = batch.lock() {
                    b.push(item.clone());
                    if b.len() >= CLEANUP_EMIT_BATCH_SIZE {
                        let _ = app_clone.emit("cleanup-item-batch", &*b);
                        b.clear();
                    }
                }

                if let Ok(mut r) = results.lock() {
                    r.push(item);
                }
            },
            |phase| {
                if let Ok(mut b) = batch.lock() {
                    if !b.is_empty() {
                        let _ = app_clone.emit("cleanup-item-batch", &*b);
                        b.clear();
                    }
                }
                if let Ok(mut ps) = phase_start.lock() {
                    let elapsed = ps.elapsed();
                    if phase.phase > 1 {
                        log_activity_with_app(
                            &app_clone,
                            "cleanup",
                            &format!("Phase {} abgeschlossen in {:.2?}", phase.phase - 1, elapsed),
                        );
                    }
                    *ps = std::time::Instant::now();
                }
                log_activity_with_app(
                    &app_clone,
                    "cleanup",
                    &format!("Starte Phase {}: {}", phase.phase, phase.label),
                );
                let _ = app_clone.emit("cleanup-phase", &phase);
            },
            {
                let last_emit = std::sync::Arc::new(std::sync::Mutex::new(std::time::Instant::now()));
                let app_clone_prog = app_clone.clone();
                move |path: &std::path::Path| {
                    if let Ok(mut last) = last_emit.try_lock() {
                        if last.elapsed().as_millis() > CLEANUP_PROGRESS_THROTTLE_MS {
                            let _ = app_clone_prog.emit("cleanup-progress", path.to_string_lossy().into_owned());
                            *last = std::time::Instant::now();
                        }
                    }
                }
            }
        );

        if let Ok(mut b) = batch.lock() {
            if !b.is_empty() {
                let _ = app_clone.emit("cleanup-item-batch", &*b);
                b.clear();
            }
        }

        if let Ok(ps) = phase_start.lock() {
            log_activity_with_app(
                &app_clone,
                "cleanup",
                &format!("Letzte Phase abgeschlossen in {:.2?}", ps.elapsed()),
            );
        }

        log_activity_with_app(
            &app_clone,
            "cleanup",
            &format!("Gesamter Scan abgeschlossen in {:.2?}", start_time.elapsed()),
        );

        if ai_assist {
            log_activity_with_app(
                &app_clone,
                "cleanup",
                &format!(
                    "KI-Auswertung: {} geprüft, {} herabgestuft, {} nicht erreichbar, {} Cache-Treffer",
                    ai_checked.load(std::sync::atomic::Ordering::Relaxed),
                    ai_downgraded.load(std::sync::atomic::Ordering::Relaxed),
                    ai_unavailable.load(std::sync::atomic::Ordering::Relaxed),
                    ai_cache_hits.load(std::sync::atomic::Ordering::Relaxed)
                ),
            );
        }

        // Sort by size (largest first)
        let mut results = results.into_inner().unwrap_or_default();
        results.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        results
    })
    .await
    .map_err(|e| format!("Cleanup scan thread panicked: {}", e))?;

    let total_size: u64 = items.iter().map(|i| i.size_bytes).sum();
    log_activity_with_app(
        &app,
        "cleanup",
        &format!(
            "Cleanup-Scan fertig — {} Einträge, {} gesamt",
            items.len(),
            crate::util::format_size(total_size)
        ),
    );
    Ok(items)
}

/// Delete selected paths. Returns per-path success/failure.
/// Runs deletions in a blocking task to avoid freezing the UI.
#[tauri::command]
pub async fn delete_cleanup_paths(
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<Vec<DeleteResult>, String> {
    if paths.is_empty() {
        return Err("Keine Pfade zum Löschen ausgewählt".to_string());
    }

    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(format!("/home/{}", username)));

    log_activity_with_app(
        &app,
        "cleanup",
        &format!("Lösche {} Einträge...", paths.len()),
    );

    let home_canon = std::fs::canonicalize(&home)
        .map_err(|e| format!("Home-Verzeichnis nicht auflösbar: {}", e))?;

    let app_for_task = app.clone();
    let results = tokio::task::spawn_blocking(move || {
        paths
            .into_par_iter()
            .map(|rel_path| {
                let abs = home_canon.join(&rel_path);

                // Safety: reject ".." traversal to prevent escaping home
                if rel_path.contains("..") {
                    return DeleteResult {
                        path: rel_path,
                        success: false,
                        error: Some("Pfad liegt außerhalb des Home-Verzeichnisses".to_string()),
                    };
                }

                // Don't delete the home dir itself or critical dotfiles/dirs
                // .ssh/.gnupg: entire tree protected (secrets)
                // .config/.local: only the dir itself, subdirs are fair game
                let tree_protected = [".ssh", ".gnupg"];
                let dir_protected = [".config", ".local", ".bashrc", ".profile", ".bash_history"];
                if abs == home_canon
                    || rel_path == "."
                    || rel_path == ".."
                    || tree_protected
                        .iter()
                        .any(|p| rel_path == *p || rel_path.starts_with(&format!("{}/", p)))
                    || dir_protected.iter().any(|p| rel_path == *p)
                {
                    return DeleteResult {
                        path: rel_path,
                        success: false,
                        error: Some("Geschützter Pfad — darf nicht gelöscht werden".to_string()),
                    };
                }

                let result = if abs.is_dir() {
                    // Some tools (e.g. Go modules) set files read-only.
                    // Make everything writable first so remove_dir_all succeeds.
                    fn make_writable_recursive(dir: &std::path::Path) {
                        use std::os::unix::fs::PermissionsExt;

                        if let Ok(entries) = std::fs::read_dir(dir) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if let Ok(meta) = path.metadata() {
                                    let mut perms = meta.permissions();
                                    if perms.readonly() {
                                        // Only ensure user-writable; don't accidentally grant world write.
                                        perms.set_mode(perms.mode() | 0o200);
                                        let _ = std::fs::set_permissions(&path, perms);
                                    }
                                    if meta.is_dir() {
                                        make_writable_recursive(&path);
                                    }
                                }
                            }
                        }
                    }
                    make_writable_recursive(&abs);
                    std::fs::remove_dir_all(&abs)
                } else if abs.is_file() || abs.is_symlink() {
                    std::fs::remove_file(&abs)
                } else {
                    return DeleteResult {
                        path: rel_path,
                        success: true,
                        error: None,
                    };
                };

                match result {
                    Ok(()) => {
                        log_activity_with_app(
                            &app_for_task,
                            "cleanup",
                            &format!("Gelöscht: {}", rel_path),
                        );
                        DeleteResult {
                            path: rel_path,
                            success: true,
                            error: None,
                        }
                    }
                    Err(e) => {
                        let err_msg = format!("{}", e);
                        log_activity_with_app(
                            &app_for_task,
                            "cleanup",
                            &format!("Fehler beim Löschen von {}: {}", rel_path, err_msg),
                        );
                        DeleteResult {
                            path: rel_path,
                            success: false,
                            error: Some(err_msg),
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("Delete thread panicked: {}", e))?;

    let ok_count = results.iter().filter(|r| r.success).count();
    let fail_count = results.iter().filter(|r| !r.success).count();
    log_activity_with_app(
        &app,
        "cleanup",
        &format!(
            "Löschvorgang abgeschlossen — {} gelöscht, {} fehlgeschlagen",
            ok_count, fail_count
        ),
    );

    Ok(results)
}

/// Determine if a category is safe to auto-select for deletion.
fn is_safe_to_delete(
    category: &crate::scanner::types::ExcludeCategory,
    path: &str,
    home: &std::path::Path,
) -> bool {
    use crate::scanner::types::ExcludeCategory;

    // Never auto-delete the top-level .cache — it contains KDE icon-caches,
    // ksycoca, plasmashell state etc. that break the desktop until re-login.
    // Sub-paths inside .cache (found by the scanner) are fine.
    if path == ".cache" {
        return false;
    }

    match category {
        ExcludeCategory::Cache | ExcludeCategory::Browser => true,
        ExcludeCategory::BuildArtifact => {
            // Only mark clearly identifiable build artifacts as safe.
            // /build and /dist are NOT auto-safe — too many false positives.
            let p = path.to_lowercase();
            (p.ends_with("/node_modules") && !is_runtime_extension_path(path))
                || p.contains("/node_modules/")
                || (p.ends_with("/target") && has_project_marker(path, "Cargo.toml", home))
                || p.contains("/__pycache__")
                || p.ends_with("/__pycache__")
                || p.ends_with("/.gradle")
                || p.ends_with("/.next")
                || p.ends_with("/.nuxt")
                || p.ends_with("/.angular")
                || p.ends_with("/.parcel-cache")
                || p.ends_with("/.turbo")
        }
        ExcludeCategory::Toolchain
        | ExcludeCategory::Runtime
        | ExcludeCategory::Gaming
        | ExcludeCategory::Container
        | ExcludeCategory::VirtualMachine
        | ExcludeCategory::Media
        | ExcludeCategory::Communication
        | ExcludeCategory::LargeUnknown => false,
    }
}

fn is_runtime_extension_path(rel_path: &str) -> bool {
    let segments: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    let Some(ext_idx) = segments.iter().position(|s| *s == "extensions") else { return false };
    let Some(nm_idx) = segments.iter().rposition(|s| *s == "node_modules") else { return false };
    if ext_idx >= nm_idx {
        return false;
    }

    // Generic heuristic: hidden/system segment before "extensions" usually
    // indicates runtime-managed extension store (e.g. .vscode-server).
    segments[..ext_idx].iter().any(|s| s.starts_with('.'))
}

fn should_ai_review_item(
    rel_path: &str,
    category: &crate::scanner::types::ExcludeCategory,
    size_bytes: u64,
) -> bool {
    use crate::scanner::types::ExcludeCategory;

    if *category != ExcludeCategory::BuildArtifact {
        return false;
    }

    let p = rel_path.to_lowercase();
    let has_hidden_segment = rel_path.split('/').any(|s| s.starts_with('.'));
    let has_extensions = p.contains("/extensions/");
    let runtime_like = p.contains(".vscode")
        || p.contains(".vscode-server")
        || p.contains("code-server")
        || p.contains("/plugin/")
        || p.contains("/plugins/")
        || p.contains("/runtime/");

    // Also review very large build artifacts to keep some AI validation active
    // without exploding request count.
    let very_large_build = size_bytes >= AI_REVIEW_SIZE_THRESHOLD;

    // Only send ambiguous/risky build artifacts (or very large ones) to AI.
    ((p.ends_with("/node_modules") || p.contains("/node_modules/"))
        && (has_hidden_segment
            || has_extensions
            || runtime_like
            || is_runtime_extension_path(rel_path)))
        || very_large_build
}

/// Check if the parent of a build directory contains a marker file.
/// `rel_path` is relative to home, so we prepend `home` for fs access.
fn has_project_marker(rel_path: &str, marker: &str, home: &std::path::Path) -> bool {
    let abs = home.join(rel_path);
    if let Some(parent) = abs.parent() {
        parent.join(marker).exists()
    } else {
        false
    }
}

#[derive(Clone)]
struct AiReviewDecision {
    downgrade: bool,
    confidence: f64,
    note: String,
    from_cache: bool,
}

enum AiReviewOutcome {
    Reviewed(AiReviewDecision),
    NoJudgment,
    Unavailable,
}

static AI_REVIEW_CACHE: OnceLock<Mutex<HashMap<String, AiReviewDecision>>> = OnceLock::new();

fn ai_cache() -> &'static Mutex<HashMap<String, AiReviewDecision>> {
    AI_REVIEW_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ai_review_item(
    rel_path: &str,
    category: &crate::scanner::types::ExcludeCategory,
    reason: &str,
    home: &std::path::Path,
) -> AiReviewOutcome {
    use std::time::Duration;

    let endpoint = std::env::var("BACKSNAP_LLM_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:8080/v1/chat/completions".to_string());
    let model = std::env::var("BACKSNAP_LLM_MODEL").unwrap_or_else(|_| "local-model".to_string());
    let timeout_ms: u64 = std::env::var("BACKSNAP_LLM_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2500);
    let retries: u8 = std::env::var("BACKSNAP_LLM_RETRIES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let abs = home.join(rel_path);
    let basename = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent_name = abs
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let has_git = has_marker_upwards(&abs, home, ".git", 6);
    let has_pkg = has_marker_upwards(&abs, home, "package.json", 6);
    let has_cargo = has_marker_upwards(&abs, home, "Cargo.toml", 6);
    let has_py = has_marker_upwards(&abs, home, "pyproject.toml", 6)
        || has_marker_upwards(&abs, home, "requirements.txt", 6);
    let has_hidden_segment = rel_path.split('/').any(|s| s.starts_with('.'));

    let cache_key = format!(
        "{}|{:?}|{}|{}|{}|{}|{}|{}",
        rel_path, category, reason, has_git, has_pkg, has_cargo, has_py, has_hidden_segment
    );
    if let Ok(cache) = ai_cache().lock() {
        if let Some(hit) = cache.get(&cache_key) {
            let mut decision = hit.clone();
            decision.from_cache = true;
            return AiReviewOutcome::Reviewed(decision);
        }
    }

    let prompt = json!({
        "task": "Decide if this candidate should be reviewed as unsafe despite deterministic safe classification.",
        "rules": [
            "Be conservative.",
            "If unsure, return downgrade=false.",
            "Only suggest downgrade when path likely belongs to runtime internals, extension payload, or non-regenerable user data.",
            "confidence = how sure you are that the path IS SAFE to delete (1.0 = definitely safe, 0.0 = definitely not safe).",
            "If downgrade=true, confidence should be LOW (you believe it is NOT safe).",
            "If downgrade=false, confidence should be HIGH (you believe it IS safe)."
        ],
        "item": {
            "relative_path": rel_path,
            "category": format!("{:?}", category),
            "reason": reason,
            "basename": basename,
            "parent": parent_name,
            "signals": {
                "has_git_upwards": has_git,
                "has_package_json_upwards": has_pkg,
                "has_cargo_toml_upwards": has_cargo,
                "has_python_marker_upwards": has_py,
                "has_hidden_path_segment": has_hidden_segment
            }
        },
        "output_format": {
            "json": {
                "downgrade": "boolean — true if NOT safe, false if safe",
                "confidence": "number 0..1 — how confident you are the path is SAFE to delete (1.0=definitely safe, 0.0=definitely unsafe)",
                "note": "short German explanation"
            }
        }
    })
    .to_string();

    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
    else {
        return AiReviewOutcome::Unavailable;
    };

    let strict_preamble = "Return ONLY one JSON object with EXACT keys: downgrade (boolean), confidence (number 0..1 = how sure path is SAFE to delete), note (string). No markdown, no code fences, no explanations.";
    let primary_prompt = format!("{}\n{}", strict_preamble, prompt);

    let Some(mut content) = request_llm_text(&client, &endpoint, &model, &primary_prompt) else { return AiReviewOutcome::Unavailable };

    let mut parsed = parse_json_object(&content);
    if parsed.is_none() {
        for _ in 0..retries {
            let repair_prompt = format!(
                "{}\nTransform this into valid JSON with exactly the required keys and valid types.\nINPUT:\n{}",
                strict_preamble,
                content
            );
            if let Some(repaired) = request_llm_text(&client, &endpoint, &model, &repair_prompt) {
                content = repaired;
                parsed = parse_json_object(&content);
                if parsed.is_some() {
                    break;
                }
            }
        }
    }

    let Some(parsed) = parsed else { return AiReviewOutcome::NoJudgment };

    let Some(downgrade) = parsed.get("downgrade").and_then(serde_json::Value::as_bool) else { return AiReviewOutcome::NoJudgment };
    let confidence = match parsed.get("confidence").and_then(serde_json::Value::as_f64) {
        Some(v) if (0.0..=1.0).contains(&v) => v,
        _ => return AiReviewOutcome::NoJudgment,
    };
    let note = match parsed.get("note").and_then(|v| v.as_str()) {
        Some(v) if !v.trim().is_empty() => v.trim(),
        _ => return AiReviewOutcome::NoJudgment,
    };

    let decision = AiReviewDecision {
        downgrade,
        confidence,
        note: note.to_string(),
        from_cache: false,
    };

    if let Ok(mut cache) = ai_cache().lock() {
        cache.insert(cache_key, decision.clone());
    }

    AiReviewOutcome::Reviewed(decision)
}

fn request_llm_text(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    model: &str,
    prompt: &str,
) -> Option<String> {
    let chat_body = json!({
        "model": model,
        "temperature": 0.0,
        "max_tokens": 120,
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "cleanup_decision",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "downgrade": { "type": "boolean" },
                        "confidence": { "type": "number" },
                        "note": { "type": "string" }
                    },
                    "required": ["downgrade", "confidence", "note"],
                    "additionalProperties": false
                }
            }
        },
        "messages": [
            {
                "role": "system",
                "content": "You are a strict file-cleanup safety classifier."
            },
            {
                "role": "user",
                "content": prompt
            }
        ]
    });

    let completions_body = json!({
        "model": model,
        "temperature": 0.0,
        "max_tokens": 120,
        "prompt": prompt
    });

    let llama_completion_body = json!({
        "prompt": prompt,
        "temperature": 0.0,
        "n_predict": 120,
        "stop": ["\n\n"]
    });

    let mut attempts: Vec<(String, serde_json::Value)> = Vec::new();
    attempts.push((endpoint.to_string(), chat_body.clone()));

    let base = llm_endpoint_base(endpoint);
    attempts.push((format!("{}/v1/chat/completions", base), chat_body));
    attempts.push((format!("{}/v1/completions", base), completions_body));
    attempts.push((format!("{}/completion", base), llama_completion_body));

    let mut unique: Vec<(String, serde_json::Value)> = Vec::new();
    for (url, body) in attempts {
        if !unique.iter().any(|(u, _)| u == &url) {
            unique.push((url, body));
        }
    }

    for (url, body) in unique {
        let Ok(response) = client.post(&url).json(&body).send() else { continue };
        if !response.status().is_success() {
            continue;
        }
        let value: serde_json::Value = match response.json() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(c) = extract_llm_text(&value) {
            return Some(c);
        }
    }

    None
}

fn llm_endpoint_base(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/v1/chat/completions")
        .trim_end_matches("/v1/completions")
        .trim_end_matches("/completion")
        .to_string()
}

fn extract_llm_text(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
    {
        return Some(s.to_string());
    }

    if let Some(s) = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c0| c0.get("text"))
        .and_then(|c| c.as_str())
    {
        return Some(s.to_string());
    }

    if let Some(s) = value.get("content").and_then(|c| c.as_str()) {
        return Some(s.to_string());
    }

    None
}

fn parse_json_object(text: &str) -> Option<serde_json::Value> {
    let normalized = normalize_llm_text(text);

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&normalized) {
        return Some(v);
    }
    let start = normalized.find('{')?;
    let end = normalized.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&normalized[start..=end]).ok()
}

fn normalize_llm_text(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(inner) = trimmed
        .strip_prefix("```json")
        .and_then(|s| s.strip_suffix("```"))
    {
        return inner.trim().to_string();
    }
    if let Some(inner) = trimmed
        .strip_prefix("```")
        .and_then(|s| s.strip_suffix("```"))
    {
        return inner.trim().to_string();
    }
    trimmed.to_string()
}

fn has_marker_upwards(
    path: &std::path::Path,
    home: &std::path::Path,
    marker: &str,
    max_up: usize,
) -> bool {
    let mut cur = path.parent();
    let mut steps = 0usize;
    while let Some(dir) = cur {
        if dir.join(marker).exists() {
            return true;
        }
        if dir == home || steps >= max_up {
            break;
        }
        cur = dir.parent();
        steps += 1;
    }
    false
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct DirEntry {
    pub name: String,
    pub path: String, // relative to home
    pub is_dir: bool,
    pub size_bytes: u64,
}

#[tauri::command]
pub async fn get_cleanup_dir_contents(
    _app: tauri::AppHandle,
    rel_path: String,
) -> Result<Vec<DirEntry>, String> {
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from(format!("/home/{}", username)));
    let home_canon = std::fs::canonicalize(&home)
        .map_err(|e| format!("Home-Verzeichnis nicht auflösbar: {}", e))?;

    let abs = home_canon.join(&rel_path);

    if rel_path.contains("..") {
        return Err("Pfad liegt außerhalb des Home-Verzeichnisses".to_string());
    }

    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&abs) {
        for entry in read_dir.flatten() {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
            let size_bytes = meta.as_ref().map_or(0, std::fs::Metadata::len);
            let name = entry.file_name().to_string_lossy().into_owned();

            // Construct the new relative path
            let child_rel = if rel_path.is_empty() || rel_path == "." {
                name.clone()
            } else {
                format!("{}/{}", rel_path, name)
            };

            entries.push(DirEntry {
                name,
                path: child_rel,
                is_dir,
                size_bytes,
            });
        }
    }

    // Sort directories first, then alphabetically
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));

    Ok(entries)
}
