//! Exclude scanner: detects excludable paths in the user's home directory.
//!
//! Architecture:
//! - `types` — shared data types (`ScannedExclude`, `ScanPhase`, `ExcludeCategory`)
//! - `walker` — native parallel filesystem walker (replaces `du`/`find`)
//! - `patterns` — known exclude‐pattern table (Phase 1)
//! - `discovery` — deep unknown‐directory discovery + classification (Phase 2)
//! - `artifacts` — build artifact scanner (Phase 3)
//! - `engine` — orchestrates all three phases, streaming or batch

mod artifacts;
mod discovery;
mod engine;
mod patterns;
pub mod types;
mod walker;

pub use engine::scan_home_excludes_streaming;

pub static CANCEL_SCAN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
