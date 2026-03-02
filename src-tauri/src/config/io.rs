//! Config I/O: load/save configuration from/to TOML files.
//!
//! Includes an in-memory cache that avoids re-reading the TOML on every
//! Tauri command. The cache is invalidated on save/reset.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::detect::auto_detect_config;
use super::types::AppConfig;

// ─── Config Cache ─────────────────────────────────────────────

static CONFIG_CACHE: OnceLock<Mutex<Option<AppConfig>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<AppConfig>> {
    CONFIG_CACHE.get_or_init(|| Mutex::new(None))
}

/// Invalidate the in-memory config cache so the next `load_config` re-reads
/// the TOML file. Call this after saving or resetting the config.
pub fn invalidate_config_cache() {
    if let Ok(mut guard) = cache().lock() {
        *guard = None;
    }
}

/// Pre-populate the config cache with a known config.
/// Used by the elevated subprocess to use the calling user's config
/// instead of auto-detecting from root's home directory.
pub fn set_config_cache(config: AppConfig) {
    if let Ok(mut guard) = cache().lock() {
        *guard = Some(config);
    }
}

// ─── Config Path ──────────────────────────────────────────────

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("/etc"))
        .join("backsnap")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

// ─── Load / Save ──────────────────────────────────────────────

pub fn load_config() -> Result<AppConfig, String> {
    // Return cached config if available
    if let Ok(guard) = cache().lock() {
        if let Some(ref cached) = *guard {
            return Ok(cached.clone());
        }
    }
    let config = load_config_from(&config_path())?;
    // Store in cache
    if let Ok(mut guard) = cache().lock() {
        *guard = Some(config.clone());
    }
    Ok(config)
}

pub fn load_config_from(path: &std::path::Path) -> Result<AppConfig, String> {
    if !path.exists() {
        let config = auto_detect_config();
        save_config_to(path, &config)?;
        return Ok(config);
    }
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Config lesen fehlgeschlagen ({}): {}", path.display(), e))?;
    toml::from_str(&content).map_err(|e| format!("Config-Fehler in {}: {}", path.display(), e))
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    save_config_to(&config_path(), config)
}

/// Atomic save: write to a temp file, then rename into place.
/// Prevents data loss if the process crashes mid-write.
pub fn save_config_to(path: &std::path::Path, config: &AppConfig) -> Result<(), String> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(dir)
        .map_err(|e| format!("Konnte {} nicht erstellen: {}", dir.display(), e))?;
    let content =
        toml::to_string_pretty(config).map_err(|e| format!("Config serialisieren: {}", e))?;
    let tmp_path = path.with_extension("toml.tmp");
    fs::write(&tmp_path, &content)
        .map_err(|e| format!("Config schreiben (tmp {}): {}", tmp_path.display(), e))?;
    fs::rename(&tmp_path, path).map_err(|e| {
        format!(
            "Config rename ({} → {}): {}",
            tmp_path.display(),
            path.display(),
            e
        )
    })?;
    invalidate_config_cache();
    Ok(())
}
