//! Snapper snapshot commands: list, create, delete, diff, subvolumes.

use super::helpers::*;
use chrono::{Local, NaiveDateTime};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

#[tauri::command]
pub async fn get_subvolumes() -> Result<Vec<SubvolumeInfo>, String> {
    tokio::task::spawn_blocking(|| {
        let result = run_privileged("btrfs", &["subvolume", "list", "-t", "/"]);
        if !result.success {
            return Err(format!("btrfs subvolume list: {}", result.stderr));
        }

        let mut subvols = Vec::new();
        for line in result.stdout.lines().skip(2) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() >= 4 {
                subvols.push(SubvolumeInfo {
                    id: cols[0].to_string(),
                    gen: cols[1].to_string(),
                    top_level: cols[2].to_string(),
                    path: cols[3..].join(" "),
                });
            }
        }
        Ok(subvols)
    })
    .await
    .map_err(|e| format!("Subvolume-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn get_snapshots(config: String) -> Result<Vec<Snapshot>, String> {
    validate_config(&config)?;
    tokio::task::spawn_blocking(move || {
        let result = run_cmd("snapper", &["--csvout", "-c", &config, "list"]);
        if !result.success {
            return Err(format!("snapper error: {}", result.stderr));
        }

        let mut snapshots = Vec::new();
        let mut lines = result.stdout.lines();
        let header = lines.next().unwrap_or_default();
        let headers: Vec<&str> = header.split(',').collect();

        let idx = |name: &str| headers.iter().position(|h| h.trim() == name);
        let i_num = idx("number").or_else(|| idx("#")).unwrap_or_default();
        let i_type = idx("type").unwrap_or(1);
        let i_pre = idx("pre-number").unwrap_or(2);
        let i_date = idx("date").unwrap_or(3);
        let i_user = idx("user").unwrap_or(4);
        let i_cleanup = idx("cleanup").unwrap_or(5);
        let i_desc = idx("description").unwrap_or(6);

        for line in lines {
            let cols: Vec<&str> = line.splitn(headers.len().max(1), ',').collect();
            if cols.len() < 4 {
                continue;
            }
            let get = |i: usize| {
                cols.get(i)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default()
            };
            let id = get(i_num).parse::<u32>().unwrap_or_default();
            if id == 0 {
                continue;
            }
            let Some(snap_type) = SnapType::parse(&get(i_type)) else { continue };
            snapshots.push(Snapshot {
                id,
                snap_type,
                pre_id: get(i_pre).parse::<u32>().ok(),
                date: get(i_date),
                user: get(i_user),
                cleanup: get(i_cleanup),
                description: get(i_desc),
            });
        }
        snapshots.reverse();
        Ok(snapshots)
    })
    .await
    .map_err(|e| format!("Snapshot-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn create_snapshot(config: String, description: String) -> Result<CommandResult, String> {
    validate_config(&config)?;
    validate_description(&description)?;
    tokio::task::spawn_blocking(move || {
        Ok(run_cmd(
            "snapper",
            &["-c", &config, "create", "-d", &description],
        ))
    })
    .await
    .map_err(|e| format!("Snapshot-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn delete_snapshot(config: String, id: u32) -> Result<CommandResult, String> {
    validate_config(&config)?;
    tokio::task::spawn_blocking(move || {
        let id_str = id.to_string();
        Ok(run_privileged(
            "snapper",
            &["-c", &config, "delete", &id_str],
        ))
    })
    .await
    .map_err(|e| format!("Snapshot-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn get_snapper_diff(config: String, id: u32) -> Result<String, String> {
    validate_config(&config)?;
    tokio::task::spawn_blocking(move || {
        let id_str = id.to_string();
        let range = format!("{}..0", id_str);
        let result = run_cmd("snapper", &["-c", &config, "status", &range]);
        if result.success {
            Ok(result.stdout)
        } else {
            Err(result.stderr)
        }
    })
    .await
    .map_err(|e| format!("Diff-Thread panicked: {}", e))?
}

// ─── Snapper Retention / Cleanup ─────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct SnapperLimits {
    pub config: String,
    /// Selected `snapper get-config` values (TIMELINE_* / NUMBER_*).
    pub values: BTreeMap<String, String>,
}

#[tauri::command]
pub async fn get_snapper_limits(config: String) -> Result<SnapperLimits, String> {
    validate_config(&config)?;
    tokio::task::spawn_blocking(move || {
        if !cmd_exists("snapper") {
            return Err("snapper nicht installiert".to_string());
        }

        // `get-config` usually requires root; use run_privileged.
        let result = run_privileged("snapper", &["-c", &config, "get-config"]);
        if !result.success {
            return Err(format!("snapper get-config: {}", result.stderr.trim()));
        }

        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for line in result.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // snapper get-config prints either KEY=VALUE (rare) or a table:
            // "Key │ Value" / "────┼────" / "TIMELINE_CLEANUP │ yes".
            let kv: Option<(String, String)> = if let Some((k, v)) = line.split_once('=') {
                Some((k.trim().to_string(), v.trim().to_string()))
            } else if line.contains('│') {
                let parts: Vec<&str> = line.split('│').map(str::trim).collect();
                if parts.len() >= 2 {
                    let k = parts[0];
                    let v = parts[1];
                    // Skip headers / separators
                    if k.eq_ignore_ascii_case("key") || k.starts_with('─') {
                        None
                    } else {
                        Some((k.to_string(), v.to_string()))
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let Some((k, mut v)) = kv else {
                continue;
            };

            let key = k.trim().to_string();
            if !(key.starts_with("TIMELINE_") || key.starts_with("NUMBER_")) {
                continue;
            }

            v = v.trim().to_string();
            if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                v = v.trim_matches('"').to_string();
            }

            values.insert(key, v);
        }

        Ok(SnapperLimits { config, values })
    })
    .await
    .map_err(|e| format!("Snapper-Thread panicked: {}", e))?
}

#[tauri::command]
pub async fn run_snapper_cleanup(config: String) -> Result<CommandResult, String> {
    validate_config(&config)?;
    tokio::task::spawn_blocking(move || {
        if !cmd_exists("snapper") {
            return Err("snapper nicht installiert".to_string());
        }

        fn list_snapshot_ids(config: &str) -> Result<BTreeSet<u32>, String> {
            let result = run_privileged("snapper", &["--csvout", "-c", config, "list"]);
            if !result.success {
                return Err(format!("snapper list: {}", result.stderr.trim()));
            }

            let mut ids: BTreeSet<u32> = BTreeSet::new();
            let mut lines = result.stdout.lines();
            let header = lines.next().unwrap_or_default();
            let headers: Vec<&str> = header.split(',').collect();
            let idx_num = headers
                .iter()
                .position(|h| h.trim() == "number" || h.trim() == "#")
                .unwrap_or_default();

            for line in lines {
                // CSV: description may contain commas; keep split bounded.
                let cols: Vec<&str> = line.splitn(headers.len().max(1), ',').collect();
                let Some(cell) = cols.get(idx_num) else {
                    continue;
                };
                let id = cell.trim().parse::<u32>().unwrap_or_default();
                if id != 0 {
                    ids.insert(id);
                }
            }
            Ok(ids)
        }

        let before = list_snapshot_ids(&config).unwrap_or_default();

        // User-requested behavior: keep only snapshots from *today*.
        // This is more aggressive than snapper's built-in cleanup limits.
        let today = Local::now().date_naive();
        let mut to_delete: Vec<u32> = Vec::new();
        {
            let result = run_privileged("snapper", &["--csvout", "-c", &config, "list"]);
            if !result.success {
                return Err(format!("snapper list: {}", result.stderr.trim()));
            }

            let mut lines = result.stdout.lines();
            let header = lines.next().unwrap_or_default();
            let headers: Vec<&str> = header.split(',').collect();
            let idx = |name: &str| headers.iter().position(|h| h.trim() == name);
            let i_num = idx("number").or_else(|| idx("#")).unwrap_or_default();
            let i_date = idx("date").unwrap_or(3);

            for line in lines {
                // CSV: description may contain commas; keep split bounded.
                let cols: Vec<&str> = line.splitn(headers.len().max(1), ',').collect();
                let get = |i: usize| cols.get(i).map_or("", |s| s.trim());

                let id = get(i_num).parse::<u32>().unwrap_or_default();
                if id == 0 {
                    continue;
                }

                let date_str = get(i_date);
                let Ok(dt) = NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S") else {
                    // If we can't parse the date reliably, keep the snapshot.
                    continue;
                };

                if dt.date() != today {
                    to_delete.push(id);
                }
            }
        }

        to_delete.sort_unstable();
        to_delete.dedup();

        let mut out_lines: Vec<String> = Vec::new();
        let mut err_lines: Vec<String> = Vec::new();
        let mut any_failed = false;
        let mut last_exit = 0;

        if to_delete.is_empty() {
            out_lines.push(format!("keep-today: nichts zu löschen (heute: {})", today));
        } else {
            // Delete in chunks to avoid long argv.
            const CHUNK: usize = 50;
            let total = to_delete.len();
            let mut deleted_attempted = 0usize;
            for chunk in to_delete.chunks(CHUNK) {
                deleted_attempted += chunk.len();
                let mut args: Vec<String> = vec!["-c".to_string(), config.clone(), "delete".to_string()];
                args.extend(chunk.iter().map(std::string::ToString::to_string));
                let arg_refs: Vec<&str> = args.iter().map(std::string::String::as_str).collect();
                let r = run_privileged("snapper", &arg_refs);
                last_exit = r.exit_code;
                if r.success {
                    out_lines.push(format!(
                        "keep-today: gelöscht {} von {} ({}..)",
                        deleted_attempted, total, today
                    ));
                } else {
                    any_failed = true;
                    err_lines.push(format!("keep-today: FEHLER (exit={}): {}", r.exit_code, r.stderr.trim()));
                }
            }
        }

        // Follow up with snapper cleanup algorithms (best-effort) so configs stay consistent.
        for algo in ["timeline", "number", "empty-pre-post"] {
            let r = run_privileged("snapper", &["-c", &config, "cleanup", algo]);
            last_exit = r.exit_code;
            if r.success {
                out_lines.push(format!("{}: OK", algo));
            } else {
                any_failed = true;
                err_lines.push(format!(
                    "{}: FEHLER (exit={}): {}",
                    algo,
                    r.exit_code,
                    r.stderr.trim()
                ));
            }
        }

        let after = list_snapshot_ids(&config).unwrap_or_default();
        let removed: Vec<u32> = before.difference(&after).copied().collect();

        let mut summary = String::new();
        use std::fmt::Write;
        let _ = writeln!(summary, "Cleanup für '{}' abgeschlossen.", config);
        let _ = writeln!(summary, "Behalte nur Snapshots von heute: {}", today);
        let _ = writeln!(summary, "Entfernt: {} Snapshot(s).", removed.len());
        if removed.is_empty() {
            summary.push_str(
                "Hinweis: 0 gelöscht bedeutet meist: Limits nicht überschritten oder *_MIN_AGE noch nicht erreicht.\n",
            );
        } else {
            // Keep output short.
            let shown: Vec<String> = removed
                .iter()
                .rev()
                .take(25)
                .map(|id| format!("#{}", id))
                .collect();
            let _ = writeln!(summary, "Gelöscht (letzte {}): {}", shown.len(), shown.join(", "));
        }

        if !out_lines.is_empty() {
            summary.push_str("\nDurchläufe:\n");
            summary.push_str(&out_lines.join("\n"));
            summary.push('\n');
        }

        Ok(CommandResult {
            success: !any_failed,
            stdout: summary.trim_end().to_string(),
            stderr: err_lines.join("\n"),
            exit_code: if any_failed { last_exit } else { 0 },
        })
    })
    .await
    .map_err(|e| format!("Cleanup-Thread panicked: {}", e))?
}
