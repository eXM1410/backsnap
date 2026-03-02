//! Shared data types for the exclude scanner.

use serde::{Deserialize, Serialize};

/// Progress info emitted at the start of each scan phase.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScanPhase {
    pub phase: u8,
    pub label: String,
}

/// A detected excludable path with metadata for the UI.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScannedExclude {
    pub path: String,
    pub category: ExcludeCategory,
    pub reason: String,
    pub size_bytes: u64,
    pub size_human: String,
    pub auto_exclude: bool,
}

/// Category of an excludable path.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ExcludeCategory {
    Cache,
    BuildArtifact,
    Toolchain,
    Gaming,
    Container,
    VirtualMachine,
    Runtime,
    Media,
    Browser,
    Communication,
    LargeUnknown,
}

/// Minimum size (in bytes) to flag unknown directories.
pub const SCAN_MIN_SIZE: u64 = 100 * 1024 * 1024; // 100 MB

/// Minimum size for unknown dirs flagged as `LargeUnknown`.
pub const LARGE_UNKNOWN_MIN: u64 = 500 * 1024 * 1024; // 500 MB
