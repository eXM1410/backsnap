//! Boot entry patching (systemd-boot only).
//!
//! Copies boot entries from Primary ESP → Backup ESP with UUID swap so the
//! backup disk is self-bootable.
//!
//! All filesystem access goes through `read_privileged` / `write_privileged` /
//! `run_privileged` so the code works both as root and as a normal user
//! (via pkexec escalation).

use super::fstab::{read_privileged, write_privileged};
use super::helpers::*;
use std::collections::HashSet;

// ─── Helpers ──────────────────────────────────────────────────

/// Rewrite `(OldLabel)` → `(NewLabel)` in a title line, or append `(Label)`.
fn relabel_entry(content: &str, new_label: &str) -> String {
    content
        .lines()
        .map(|line| {
            if line.starts_with("title ") {
                if let (Some(s), Some(e)) = (line.find('('), line.rfind(')')) {
                    if e > s {
                        return format!("{}({}){}", &line[..s], new_label, &line[e + 1..]);
                    }
                }
                format!("{} ({})", line, new_label)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sanitize_label(label: &str, max_len: usize) -> String {
    let cleaned: String = label
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect();
    if cleaned.len() > max_len {
        cleaned[..max_len].trim().to_string()
    } else {
        cleaned
    }
}

/// Insert or update a `sort-key` line (keeps cross-entries sorted after local ones).
fn upsert_sort_key(content: &str, sort_key: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    if let Some(i) = lines.iter().position(|l| l.starts_with("sort-key ")) {
        lines[i] = format!("sort-key {}", sort_key);
    } else {
        let at = lines.iter().position(|l| l.starts_with("title ")).map_or(0, |i| i + 1);
        lines.insert(at, format!("sort-key {}", sort_key));
    }
    lines.join("\n")
}

/// Sort-key: LTS(1) → normal(2) → rescue(8), cross-entries get `9` prefix.
fn entry_sort_key(base: &str, cross: bool) -> String {
    let n = if base.contains("-lts") { "1" } else if base.contains("-rescue") { "8" } else { "2" };
    format!("{}{n}-{base}", if cross { "9" } else { "" })
}

// ─── Public API ───────────────────────────────────────────────

/// Copy boot entries from Primary ESP → Backup ESP, replacing the btrfs UUID
/// so the backup is self-bootable.  Stale entries on the backup that no longer
/// exist on the primary are removed.
pub(super) fn patch_backup_boot_entries(
    boot_mnt: &str,
    primary_uuid: &str,
    backup_uuid: &str,
    backup_label: &str,
    log_path: &str,
) -> Result<(), String> {
    if primary_uuid.is_empty() || backup_uuid.is_empty() || primary_uuid == backup_uuid {
        return Ok(());
    }

    let short_label = sanitize_label(backup_label, 20);
    let primary_dir = "/boot/loader/entries";
    let backup_dir = format!("{}/loader/entries", boot_mnt);

    // ── Read primary entries ──────────────────────────────
    let conf_files = list_conf_files(primary_dir);
    if conf_files.is_empty() {
        let msg = format!(
            "FEHLER: Keine .conf-Dateien in {} lesbar — \
             Boot-Entries werden NICHT auf Backup kopiert! \
             Prüfe Berechtigungen oder ob /boot gemountet ist.",
            primary_dir
        );
        sync_log(log_path, &msg);
        return Err(msg);
    }

    // ── Copy + patch ──────────────────────────────────────
    let mut ok = 0u32;
    let mut names: HashSet<&str> = HashSet::new();

    for fname in &conf_files {
        names.insert(fname.as_str());

        let mut content = match read_privileged(&format!("{}/{}", primary_dir, fname)) {
            Ok(c) => c,
            Err(e) => {
                sync_log(log_path, &format!("WARNUNG: {} lesen: {}", fname, e));
                continue;
            }
        };

        // Ensure sort-key exists (survives kernel-hook overwrites)
        if !fname.contains("-cross-") {
            let with_sk = upsert_sort_key(&content, &entry_sort_key(fname.trim_end_matches(".conf"), false));
            if with_sk != content {
                let _ = write_privileged(&format!("{}/{}", primary_dir, fname), &with_sk);
                content = with_sk;
            }
        }

        let patched = relabel_entry(
            &content.replace(&format!("UUID={}", primary_uuid), &format!("UUID={}", backup_uuid)),
            &short_label,
        );

        match write_privileged(&format!("{}/{}", backup_dir, fname), &patched) {
            Ok(()) => ok += 1,
            Err(e) => sync_log(log_path, &format!("WARNUNG boot entry {}: {}", fname, e)),
        }
    }

    // ── Remove stale entries on backup (but not cross-entries) ──
    for fname in list_conf_files(&backup_dir) {
        if fname.contains("-cross-") { continue; }
        if !names.contains(fname.as_str()) {
            let path = format!("{}/{}", backup_dir, fname);
            if run_privileged("rm", &["-f", &path]).success {
                sync_log(log_path, &format!("Stale Backup-Entry entfernt: {}", fname));
            }
        }
    }

    // ── Result ────────────────────────────────────────────
    if ok == 0 {
        let msg = format!(
            "FEHLER: 0/{} Boot-Entries gepatcht. UUID {} → {}",
            conf_files.len(), primary_uuid, backup_uuid
        );
        sync_log(log_path, &msg);
        return Err(msg);
    }

    sync_log(
        log_path,
        &format!(
            "Boot-Entry-Sync OK: {}/{} Einträge (UUID: {} → {})",
            ok, conf_files.len(), primary_uuid, backup_uuid
        ),
    );
    Ok(())
}

// ─── Cross-Boot Entries ───────────────────────────────────────

/// Write cross-boot entries so each ESP's boot menu can reach the *other* disk.
/// On backup ESP: entries that boot the primary (UUID unchanged, label = primary).
/// On primary ESP: entries that boot the backup (UUID swapped, label = backup).
/// Stale cross-entries are cleaned up automatically.
pub(super) fn write_cross_boot_entries(
    backup_mnt: &str,
    primary_uuid: &str,
    backup_uuid: &str,
    primary_label: &str,
    backup_label: &str,
    log_path: &str,
) {
    let primary_esp = "/boot/loader/entries";
    let backup_esp = format!("{}/loader/entries", backup_mnt);

    let conf_files = list_conf_files(primary_esp);
    if conf_files.is_empty() {
        sync_log(log_path, &format!(
            "WARNUNG: Keine .conf in {} — Cross-Boot-Entries übersprungen.", primary_esp
        ));
        return;
    }

    let tag_primary = sanitize_label(primary_label, 40).replace(' ', "").to_lowercase();
    let tag_backup = sanitize_label(backup_label, 40).replace(' ', "").to_lowercase();

    let mut written_backup = Vec::new(); // cross-entry names written to backup ESP
    let mut written_primary = Vec::new(); // cross-entry names written to primary ESP

    for fname in &conf_files {
        if fname.contains("-cross-") { continue; }

        let Ok(content) = read_privileged(&format!("{}/{}", primary_esp, fname)) else { continue };
        if !content.contains(&format!("UUID={}", primary_uuid)) { continue; }

        let base = fname.trim_end_matches(".conf");
        let sort = entry_sort_key(base, true);

        // → Backup ESP: cross-entry that boots primary (UUID stays)
        let name = format!("{}-cross-{}.conf", base, tag_primary);
        let body = upsert_sort_key(&relabel_entry(&content, primary_label), &sort);
        if write_privileged(&format!("{}/{}", backup_esp, name), &body).is_ok() {
            written_backup.push(name);
        }

        // → Primary ESP: cross-entry that boots backup (UUID swapped)
        let name = format!("{}-cross-{}.conf", base, tag_backup);
        let swapped = content.replace(
            &format!("UUID={}", primary_uuid),
            &format!("UUID={}", backup_uuid),
        );
        let body = upsert_sort_key(&relabel_entry(&swapped, backup_label), &sort);
        if write_privileged(&format!("{}/{}", primary_esp, name), &body).is_ok() {
            written_primary.push(name);
        }
    }

    // ── Clean up stale cross-entries ──────────────────────
    let valid_backup: HashSet<_> = written_backup.iter().map(String::as_str).collect();
    let valid_primary: HashSet<_> = written_primary.iter().map(String::as_str).collect();

    for fname in list_conf_files(&backup_esp) {
        if fname.contains("-cross-") && !valid_backup.contains(fname.as_str())
            && run_privileged("rm", &["-f", &format!("{}/{}", backup_esp, fname)]).success {
                sync_log(log_path, &format!("Stale Cross-Entry entfernt (Backup): {}", fname));
            }
    }
    for fname in list_conf_files(primary_esp) {
        if fname.contains("-cross-") && !valid_primary.contains(fname.as_str())
            && run_privileged("rm", &["-f", &format!("{}/{}", primary_esp, fname)]).success {
                sync_log(log_path, &format!("Stale Cross-Entry entfernt (Primary): {}", fname));
            }
    }

    sync_log(log_path, &format!(
        "Cross-Boot-Entries: {} auf Backup-ESP, {} auf Primary-ESP",
        written_backup.len(), written_primary.len()
    ));
}
