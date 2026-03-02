//! Shared utility functions: command execution, size formatting, validation.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

// ─── Safe Command Execution ───────────────────────────────────

/// Run a command safely: stdin is closed (prevents polkit/password hangs)
/// and a 2s timeout kills the process if it takes too long.
pub fn safe_cmd(cmd: &str, args: &[&str]) -> Option<std::process::Output> {
    safe_cmd_timeout(cmd, args, Duration::from_secs(2))
}

/// Run a command with a custom timeout. Stdin is closed, stdout/stderr are piped.
pub fn safe_cmd_timeout(
    cmd: &str,
    args: &[&str],
    timeout: Duration,
) -> Option<std::process::Output> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // reap zombie
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

// ─── Size Formatting ──────────────────────────────────────────

// CAST-SAFETY: byte counts displayed as human-readable sizes; f64 precision loss is negligible
#[allow(clippy::cast_precision_loss)]
pub fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const TB: u64 = 1_099_511_627_776;
    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

pub fn parse_size_to_bytes(size: &str) -> u64 {
    let s = size.trim();
    // Try with space first ("500 GB"), then without ("500GB", "500G")
    let suffixes: &[(&[&str], f64)] = &[
        (&[" TB", "TB", "T"], 1_099_511_627_776.0),
        (&[" GB", "GB", "G"], 1_073_741_824.0),
        (&[" MB", "MB", "M"], 1_048_576.0),
        (&[" KB", "KB", "K"], 1_024.0),
    ];
    for (variants, multiplier) in suffixes {
        for suffix in *variants {
            if let Some(val) = s.strip_suffix(suffix) {
                let bytes = (val.trim().parse::<f64>().unwrap_or(0.0) * multiplier).max(0.0);
                // CAST-SAFETY: clamped ≥0 above; Rust saturating semantics handle overflow
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                return bytes as u64;
            }
        }
    }
    // Try parsing as raw bytes
    s.parse::<u64>().unwrap_or_default()
}
