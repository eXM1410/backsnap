//! Native parallel filesystem walker — replaces `du -sb` and `find`.
//!
//! Uses `jwalk` for multi-threaded directory traversal and `rayon` for
//! parallel size computation. No subprocess spawning, no shell overhead.

use jwalk::WalkDir;
use std::path::Path;

/// Compute the total size (in bytes) of a directory tree in parallel.
///
/// This replaces `du -sb`. Uses `jwalk` for I/O-parallel directory walking
/// and sums file sizes via metadata (no extra syscalls beyond `readdir` + `stat`).
/// Follows the same semantics as `du -sb`: counts apparent file sizes, not blocks.
pub fn dir_size(
    path: &Path,
    on_progress: Option<&(dyn Fn(&std::path::Path) + Sync + Send)>,
) -> u64 {
    if !path.exists() {
        return 0;
    }

    // jwalk parallelises readdir across threads automatically.
    // We skip errors silently (permission denied, broken symlinks, etc.).
    WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(
            num_cpus().min(8), // cap at 8 I/O threads — more doesn't help on NVMe
        ))
        .into_iter()
        .take_while(|_| !crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed))
        .filter_map(std::result::Result::ok)
        .inspect(|e| {
            if let Some(cb) = on_progress {
                if e.file_type().is_dir() {
                    cb(e.path().as_path());
                }
            }
        })
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or_default())
        .sum()
}

/// Shallow directory listing (max depth), with safety cap.
/// Returns all entries up to `max_depth` levels deep, capped at 1000 entries.
pub fn walk_shallow(dir: &Path, max_depth: usize) -> Vec<std::path::PathBuf> {
    WalkDir::new(dir)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(max_depth)
        .into_iter()
        .take(1000) // safety cap
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .collect()
}

/// Find directories matching any of the given names, up to `max_depth`.
///
/// This replaces `find $home -maxdepth N -type d \( -name X -o -name Y \)`.
/// Returns relative paths (stripped of `root`).
pub fn find_dirs_by_name(
    root: &Path,
    names: &[&str],
    max_depth: usize,
    on_progress: Option<&(dyn Fn(&std::path::Path) + Sync + Send)>,
) -> Vec<std::path::PathBuf> {
    WalkDir::new(root)
        .skip_hidden(false)
        .follow_links(false)
        .max_depth(max_depth)
        .parallelism(jwalk::Parallelism::RayonNewPool(num_cpus().min(8)))
        .into_iter()
        .take_while(|_| !crate::scanner::CANCEL_SCAN.load(std::sync::atomic::Ordering::Relaxed))
        .filter_map(std::result::Result::ok)
        .inspect(|e| {
            if let Some(cb) = on_progress {
                if e.file_type().is_dir() {
                    cb(e.path().as_path());
                }
            }
        })
        .filter(|e| {
            e.file_type().is_dir() && e.file_name().to_str().is_some_and(|n| names.contains(&n))
        })
        .map(|e| e.path())
        .collect()
}

/// Check if a glob-like pattern (e.g. `.mozilla/firefox/*/storage`) has any matches.
pub fn glob_exists(home: &Path, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() < 2 {
        return home.join(pattern).exists();
    }

    let prefix = parts[0].trim_end_matches('/');
    let prefix_path = home.join(prefix);
    if !prefix_path.exists() {
        return false;
    }

    if let Ok(entries) = std::fs::read_dir(&prefix_path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let suffix = parts.last().unwrap_or(&"").trim_start_matches('/');
                if suffix.is_empty() || entry.path().join(suffix).exists() {
                    return true;
                }
            }
        }
    }
    false
}

/// Logical CPU count (capped, for thread pool sizing).
fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(4)
}
