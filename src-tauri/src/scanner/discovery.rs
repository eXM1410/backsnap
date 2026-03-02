//! Phase 2: Deep directory discovery and classification.
//!
//! Scans directories not covered by known patterns and classifies them
//! by their content (game files, caches, VM data, etc.).

use std::path::Path;

use super::types::*;
use super::walker;
use crate::util::format_size;

/// Signals detected inside a directory, used for heuristic classification.
pub struct DirSignals {
    pub is_project: bool,
    pub has_source_code: bool,
    pub has_important_content: bool,
    pub game_score: u8,
    pub cache_score: u8,
    pub vm_score: u8,
    pub has_disk_images: bool,
}

impl DirSignals {
    /// Analyze a directory's immediate contents (up to depth 2) for signals.
    pub fn analyze(path: &Path) -> Self {
        let mut s = DirSignals {
            is_project: false,
            has_source_code: false,
            has_important_content: false,
            game_score: 0,
            cache_score: 0,
            vm_score: 0,
            has_disk_images: false,
        };

        // Project markers — files that indicate this is a source code project
        let project_markers = [
            ".git",
            "package.json",
            "Cargo.toml",
            "go.mod",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "setup.py",
            "pyproject.toml",
            "setup.cfg",
            "requirements.txt",
            "Pipfile",
            "Gemfile",
            "Rakefile",
            "composer.json",
            "mix.exs",
            "CMakeLists.txt",
            "Makefile",
            "justfile",
            ".sln",
            ".csproj",
            ".fsproj",
            "Dockerfile",
            "docker-compose.yml",
            "docker-compose.yaml",
            "flake.nix",
            "shell.nix",
            "angular.json",
            "tsconfig.json",
            "manage.py", // Django
        ];
        for marker in &project_markers {
            if path.join(marker).exists() {
                s.is_project = true;
                break;
            }
        }

        // Shallow scan for content signals (native, not shelling out)
        let entries = walker::walk_shallow(path, 2);
        let mut exe_count = 0u32;
        let mut dll_count = 0u32;
        let mut so_count = 0u32;
        let mut shader_count = 0u32;
        let mut large_binary_count = 0u32;
        let mut qcow_count = 0u32;
        let mut layer_count = 0u32;

        for entry in &entries {
            let ext = entry
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            match ext.as_str() {
                "exe" => exe_count += 1,
                "dll" => dll_count += 1,
                "so" => so_count += 1,
                "spv" | "dxbc" | "fxc" => shader_count += 1,
                "qcow2" | "vmdk" | "vdi" | "vhdx" | "img" | "raw" => qcow_count += 1,
                "pak" | "upk" | "uasset" | "wad" | "bsp" | "vpk" => {
                    s.game_score += 1;
                }
                // Source code files → this is a real project, not a cache
                "py" | "rs" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" | "kt" | "rb" | "cpp"
                | "c" | "h" | "cs" | "swift" | "lua" | "php" | "ex" | "exs" | "hs" | "scala"
                | "clj" | "dart" | "r" | "jl" => {
                    if entry.is_file() {
                        s.has_source_code = true;
                    }
                }
                // Database / important data files
                "db" | "sqlite" | "sqlite3" => {
                    if entry.is_file() {
                        s.has_important_content = true;
                    }
                }
                _ => {}
            }

            // Check for large binaries (>50 MB)
            if entry.is_file() {
                if let Ok(meta) = entry.metadata() {
                    if meta.len() > 50 * 1024 * 1024 {
                        large_binary_count += 1;
                    }
                }
            }

            // Docker/OCI layer signals
            if name == "layer.tar" || name == "manifest.json" || name == "repositories" {
                layer_count += 1;
            }

            // Cache signals
            if name.contains("cache") || name.contains("tmp") || name == "blobs" {
                s.cache_score += 1;
            }

            // Important content signals — config/settings/profile files & dirs
            // These indicate the directory contains user-critical data and
            // should NOT be bulk-deleted even if it also has cache subdirs.
            if entry.is_file() {
                let important_file_names = [
                    "settings.json",
                    "preferences",
                    "prefs.js",
                    "profiles.ini",
                    "bookmarks",
                    "favicons",
                    "logins.json",
                    "key4.db",
                    "cert9.db",
                    "cookies",
                    "history",
                    "local state",
                ];
                if important_file_names.iter().any(|&n| name == n) {
                    s.has_important_content = true;
                }
            } else if entry.is_dir() {
                let important_dir_names = [
                    "user",
                    "profiles",
                    "sessions",
                    "workspaces",
                    "snippets",
                    "keybindings",
                ];
                if important_dir_names.iter().any(|&n| name == n) {
                    s.has_important_content = true;
                }
            }
        }

        // Scoring
        if exe_count >= 2 {
            s.game_score += 1;
        }
        if dll_count >= 3 {
            s.game_score += 1;
        }
        if so_count >= 3 && exe_count >= 1 {
            s.game_score += 1;
        }
        if shader_count >= 2 {
            s.game_score += 1;
        }
        if large_binary_count >= 2 {
            s.game_score += 1;
        }

        if qcow_count >= 1 {
            s.vm_score += 2;
            s.has_disk_images = true;
        }
        if layer_count >= 2 {
            s.vm_score += 2;
        }

        s
    }
}

/// Classify an unknown large directory by examining its content signals.
///
/// Returns `None` if the directory looks like a normal project or is too small.
pub fn classify_unknown_dir(rel_path: &str, abs_path: &Path, size: u64) -> Option<ScannedExclude> {
    let signals = DirSignals::analyze(abs_path);

    // It's a project repo — glob patterns handle its artifacts already
    if signals.is_project {
        return None;
    }

    // Contains source code files → treat as a project, not a cache
    if signals.has_source_code {
        return None;
    }

    // Mixed content: has both cache-like dirs AND important user data
    // (e.g. .config/Code with Cache/ + User/settings.json)
    // → don't flag the entire directory, individual caches are handled by known patterns
    if signals.has_important_content && signals.cache_score > 0 {
        return None;
    }

    // Looks like game files
    if signals.game_score >= 3 {
        return Some(ScannedExclude {
            path: rel_path.to_string(),
            category: ExcludeCategory::Gaming,
            reason: format!(
                "Mögliche Spieldateien ({}) — wahrscheinlich per Launcher regenerierbar",
                format_size(size)
            ),
            size_bytes: size,
            size_human: format_size(size),
            auto_exclude: false,
        });
    }

    // Looks like a cache directory
    if signals.cache_score >= 2 {
        return Some(ScannedExclude {
            path: rel_path.to_string(),
            category: ExcludeCategory::Cache,
            reason: format!(
                "Wahrscheinlicher Cache ({}) — enthält typische Cache-Strukturen",
                format_size(size)
            ),
            size_bytes: size,
            size_human: format_size(size),
            auto_exclude: false,
        });
    }

    // Looks like container/VM data
    if signals.vm_score >= 2 {
        return Some(ScannedExclude {
            path: rel_path.to_string(),
            category: if signals.has_disk_images {
                ExcludeCategory::VirtualMachine
            } else {
                ExcludeCategory::Container
            },
            reason: format!(
                "VM/Container-Daten ({}) — enthält Disk-Images oder Container-Layer",
                format_size(size)
            ),
            size_bytes: size,
            size_human: format_size(size),
            auto_exclude: false,
        });
    }

    // Large unknown — flag for user review
    if size >= LARGE_UNKNOWN_MIN {
        return Some(ScannedExclude {
            path: rel_path.to_string(),
            category: ExcludeCategory::LargeUnknown,
            reason: format!(
                "Großes Verzeichnis ({}) — prüfe ob Backup nötig",
                format_size(size)
            ),
            size_bytes: size,
            size_human: format_size(size),
            auto_exclude: false,
        });
    }

    None
}
