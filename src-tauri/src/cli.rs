//! CLI-only privileged helpers.
//!
//! These functions run as root via pkexec and have zero Tauri dependency.
//! They accept JSON on stdin/argv and write results to stderr.

use crate::commands::helpers::FileOp;

/// Native sysfs write helper — runs as root via pkexec.
/// Expects JSON: `[{"path": "/sys/...", "value": "123"}, ...]`
#[allow(clippy::print_stderr)]
pub fn run_sysfs_write(json: &str) -> i32 {
    #[derive(serde::Deserialize)]
    struct SysWrite {
        path: String,
        value: String,
    }

    let writes: Vec<SysWrite> = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON-Fehler: {e}");
            return 1;
        }
    };

    if writes.is_empty() {
        eprintln!("Keine Writes angegeben");
        return 1;
    }

    // Security: only allow writes to /sys/, /proc/sys/, /etc/arclight/, /etc/environment, /etc/sysctl.d/
    for w in &writes {
        let allowed = w.path.starts_with("/sys/")
            || w.path.starts_with("/proc/sys/")
            || w.path.starts_with("/etc/arclight/")
            || w.path == "/etc/environment"
            || w.path.starts_with("/etc/sysctl.d/");
        if !allowed {
            eprintln!("Nicht erlaubter Pfad: {}", w.path);
            return 1;
        }
    }

    for w in &writes {
        if w.value == "__DELETE__" {
            if let Err(e) = std::fs::remove_file(&w.path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("Fehler beim Löschen {}: {e}", w.path);
                    return 1;
                }
            }
        } else if w.value == "__MKDIR__" {
            let dir = std::path::Path::new(&w.path)
                .parent()
                .unwrap_or(std::path::Path::new(&w.path));
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!("Fehler beim Erstellen {}: {e}", dir.display());
                return 1;
            }
        } else {
            // Ensure parent directory exists for config files
            if w.path.starts_with("/etc/arclight/") {
                let _ = std::fs::create_dir_all("/etc/arclight");
            }
            if w.path.starts_with("/etc/sysctl.d/") {
                let _ = std::fs::create_dir_all("/etc/sysctl.d");
            }
            if let Err(e) = std::fs::write(&w.path, &w.value) {
                eprintln!("Fehler beim Schreiben {} → {}: {e}", w.path, w.value);
                return 1;
            }
        }
    }

    0
}

/// Native privileged file-ops helper — runs as root via a single pkexec.
///
/// Accepts JSON array of operations:
/// - `{"op":"write",  "path":"...", "content":"..."}`
/// - `{"op":"copy",   "src":"...",  "dst":"..."}`
/// - `{"op":"delete", "path":"..."}`
/// - `{"op":"mkdir",  "path":"..."}`
/// - `{"op":"chmod",  "path":"...", "mode": 755}`
#[allow(clippy::print_stderr)]
pub fn run_file_ops(json: &str) -> i32 {
    use std::os::unix::fs::PermissionsExt;

    let ops: Vec<FileOp> = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON-Fehler: {e}");
            return 1;
        }
    };

    if ops.is_empty() {
        eprintln!("Keine Operationen angegeben");
        return 1;
    }

    // Security: allowed path prefixes for privileged file ops
    const ALLOWED: &[&str] = &[
        "/etc/systemd/system/",
        "/etc/polkit-1/",
        "/etc/pacman.d/",
        "/etc/arclight/",
        "/etc/environment",
        "/etc/sysctl.d/",
        "/etc/udev/rules.d/",
        "/usr/bin/arclight",
        "/usr/local/bin/arclight",
        "/usr/share/applications/arclight",
        "/usr/share/icons/hicolor/",
        "/tmp/arclight-",
        "/sys/",
        "/proc/sys/",
    ];

    let check = |p: &str| -> bool { ALLOWED.iter().any(|pfx| p.starts_with(pfx)) };

    // Security validation pass — reject any disallowed paths
    for op in &ops {
        let blocked = match op {
            FileOp::Copy { dst, .. } => !dst.is_empty() && !check(dst),
            FileOp::Write { path, .. }
            | FileOp::Delete { path }
            | FileOp::Mkdir { path }
            | FileOp::Chmod { path, .. } => !path.is_empty() && !check(path),
        };
        if blocked {
            eprintln!("Nicht erlaubter Pfad in {op:?}");
            return 1;
        }
    }

    // Execution pass
    for op in &ops {
        let result: Result<(), String> = match op {
            FileOp::Write { path, content } => {
                if let Some(parent) = std::path::Path::new(path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(path, content).map_err(|e| format!("write {path}: {e}"))
            }
            FileOp::Copy { src, dst } => {
                if let Some(parent) = std::path::Path::new(dst).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(src, dst)
                    .map(|_| ())
                    .map_err(|e| format!("copy {src} → {dst}: {e}"))
            }
            FileOp::Delete { path } => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(format!("delete {path}: {e}")),
            },
            FileOp::Mkdir { path } => {
                std::fs::create_dir_all(path).map_err(|e| format!("mkdir {path}: {e}"))
            }
            FileOp::Chmod { path, mode } => {
                let perms = std::fs::Permissions::from_mode(*mode);
                std::fs::set_permissions(path, perms)
                    .map_err(|e| format!("chmod {mode} {path}: {e}"))
            }
        };

        if let Err(e) = result {
            eprintln!("Fehler: {e}");
            return 1;
        }
    }

    0
}
