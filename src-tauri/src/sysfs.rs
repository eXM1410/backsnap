//! Native Linux filesystem introspection — replaces blkid, mountpoint, findmnt.
//!
//! All functions read directly from `/dev/disk/by-uuid/`, `/proc/self/mountinfo`,
//! and `stat()` without spawning any external processes.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

// ─── blkid replacements ──────────────────────────────────────

/// Resolve a UUID to a block device path (replaces `blkid -U <uuid>`).
///
/// Reads the symlink `/dev/disk/by-uuid/<uuid>` and canonicalizes it.
/// Returns `None` if the UUID is not found (disk not connected).
pub fn resolve_uuid(uuid: &str) -> Option<String> {
    let link = PathBuf::from(format!("/dev/disk/by-uuid/{}", uuid));
    fs::canonicalize(&link)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Look up the UUID of a block device (replaces `blkid -s UUID -o value <dev>`).
///
/// Iterates `/dev/disk/by-uuid/` symlinks to find which one points to `dev`.
pub fn device_uuid(dev: &str) -> Option<String> {
    let dev_canon = fs::canonicalize(dev).ok()?;
    let dir = fs::read_dir("/dev/disk/by-uuid/").ok()?;
    for entry in dir.flatten() {
        if let Ok(target) = fs::canonicalize(entry.path()) {
            if target == dev_canon {
                return entry.file_name().to_str().map(std::string::ToString::to_string);
            }
        }
    }
    None
}

/// Check if a UUID exists (disk is present).  Replaces `blkid -U <uuid>` success check.
pub fn uuid_exists(uuid: &str) -> bool {
    Path::new(&format!("/dev/disk/by-uuid/{}", uuid)).exists()
}

// ─── mountpoint replacement ──────────────────────────────────

/// Check whether `path` is a mount point (replaces `mountpoint -q <path>`).
///
/// A directory is a mount point if its `st_dev` differs from its parent's `st_dev`,
/// or if it is the filesystem root (`/`).
pub fn is_mountpoint(path: &str) -> bool {
    let p = Path::new(path);
    let Ok(meta) = fs::metadata(p) else { return false };
    if !meta.is_dir() {
        return false;
    }
    // The root directory is always a mount point
    if p == Path::new("/") {
        return true;
    }
    let Some(parent) = p.parent() else { return true };
    let Ok(parent_meta) = fs::metadata(parent) else { return false };
    meta.dev() != parent_meta.dev()
}

// ─── /proc/self/mountinfo parser ─────────────────────────────

/// A parsed entry from `/proc/self/mountinfo`.
#[derive(Debug, Clone)]
pub struct MountInfo {
    /// root of the mount within the filesystem (field 4) — e.g. `/@` for btrfs subvol @
    pub root: String,
    /// mount point (field 5)
    pub mount_point: String,
    /// mount options (field 6) — per-mount options like `rw,relatime`
    pub mount_options: String,
    /// filesystem type (e.g. `btrfs`, `vfat`, `ext4`)
    pub fstype: String,
    /// mount source (e.g. `/dev/nvme0n1p2`)
    pub source: String,
    /// super options (e.g. `compress=zstd:3,ssd,discard=async`)
    pub super_options: String,
}

/// Parse `/proc/self/mountinfo` into a list of `MountInfo` entries.
///
/// Format per line (from `man 5 proc`):
/// ```text
/// 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
///                                              ^^^ separator
/// ```
fn parse_mountinfo() -> Vec<MountInfo> {
    let Ok(data) = fs::read_to_string("/proc/self/mountinfo") else { return Vec::new() };
    parse_mountinfo_str(&data)
}

fn parse_mountinfo_str(data: &str) -> Vec<MountInfo> {
    let mut entries = Vec::new();
    for line in data.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }
        let root = unescape_mountinfo(parts[3]);
        let mount_point = unescape_mountinfo(parts[4]);
        let mount_options = parts[5].to_string();

        // Fields 6..N are optional fields, terminated by a single `-`
        let mut idx = 6;
        while idx < parts.len() && parts[idx] != "-" {
            idx += 1;
        }
        // Skip the `-` separator
        idx += 1;
        let fstype = (*parts.get(idx).unwrap_or(&"")).to_string();
        let source = (*parts.get(idx + 1).unwrap_or(&"")).to_string();
        let super_options = (*parts.get(idx + 2).unwrap_or(&"")).to_string();

        entries.push(MountInfo {
            root,
            mount_point,
            mount_options,
            fstype,
            source,
            super_options,
        });
    }
    entries
}

/// Unescape octal sequences in mountinfo fields (e.g. `\040` → ` `, `\011` → `\t`).
fn unescape_mountinfo(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let oct: String = chars.by_ref().take(3).collect();
            if let Ok(byte) = u8::from_str_radix(&oct, 8) {
                result.push(byte as char);
            } else {
                result.push('\\');
                result.push_str(&oct);
            }
        } else {
            result.push(c);
        }
    }
    result
}

// ─── findmnt replacements ────────────────────────────────────

/// Find the mount entry for a given path (replaces `findmnt <path> ...`).
///
/// Walks up from `path` to `/`, returning the most specific (longest) mount point
/// that matches a prefix of `path`.
pub fn find_mount(path: &str) -> Option<MountInfo> {
    let entries = parse_mountinfo();
    find_mount_in(path, &entries)
}

fn find_mount_in(path: &str, entries: &[MountInfo]) -> Option<MountInfo> {
    let clean = path.trim_end_matches('/');
    let target = if clean.is_empty() { "/" } else { clean };

    // Find the longest mount_point that is a prefix of `target`.
    // This gives us the most specific mount.
    let mut best: Option<&MountInfo> = None;
    for entry in entries {
        let mp = entry.mount_point.trim_end_matches('/');
        let mp = if mp.is_empty() { "/" } else { mp };

        if target == mp || target.starts_with(&format!("{}/", mp)) || mp == "/" {
            match &best {
                Some(b) => {
                    if mp.len() > b.mount_point.trim_end_matches('/').len() {
                        best = Some(entry);
                    }
                }
                None => best = Some(entry),
            }
        }
    }
    best.cloned()
}

/// Get the UUID of the device mounted at `path` (replaces `findmnt <path> -o UUID -n`).
pub fn mount_uuid(path: &str) -> Option<String> {
    let info = find_mount(path)?;
    device_uuid(&info.source)
}

/// Get the filesystem root (subvolume) at `path` (replaces `findmnt <path> -o FSROOT -n`).
pub fn mount_fsroot(path: &str) -> Option<String> {
    find_mount(path).map(|m| m.root)
}

/// Get mount options at `path` (replaces `findmnt <path> -o OPTIONS -n`).
///
/// Returns the combined super_options (which contain btrfs-relevant options like
/// compress, ssd, etc.)
pub fn mount_options(path: &str) -> Option<String> {
    find_mount(path).map(|m| {
        if m.super_options.is_empty() {
            m.mount_options
        } else {
            format!("{},{}", m.mount_options, m.super_options)
        }
    })
}

/// Get source device and fsroot at `path` (replaces `findmnt <path> -o SOURCE,FSROOT -n`).
pub fn mount_source_fsroot(path: &str) -> Option<(String, String)> {
    find_mount(path).map(|m| (m.source.clone(), m.root))
}

/// Find where a specific block device is currently mounted.
/// Returns the first mount point found, or `None` if the device is not mounted.
pub fn find_device_mountpoint(dev: &str) -> Option<String> {
    let entries = parse_mountinfo();
    let canonical = fs::canonicalize(dev)
        .ok()
        .map(|p| p.to_string_lossy().into_owned());

    for entry in &entries {
        // Direct match
        if entry.source == dev {
            return Some(entry.mount_point.clone());
        }
        // Canonical (symlink-resolved) match
        if let Some(ref canon) = canonical {
            if let Ok(entry_canon) = fs::canonicalize(&entry.source) {
                if entry_canon.to_string_lossy() == canon.as_str() {
                    return Some(entry.mount_point.clone());
                }
            }
        }
    }
    None
}

/// Get all child mounts below `path` (replaces `findmnt -R -J -o TARGET,SOURCE,FSTYPE <path>`).
///
/// Returns a list of (target, source, fstype) for mounts strictly below `path`.
pub fn nested_mounts(path: &str) -> Vec<(String, String, String)> {
    let entries = parse_mountinfo();
    let clean = path.trim_end_matches('/');
    let prefix = if clean.is_empty() { "/" } else { clean };

    let mut results = Vec::new();
    for entry in &entries {
        let mp = &entry.mount_point;
        // Must be strictly below `prefix` (not equal)
        if mp != prefix
            && mp.starts_with(prefix)
            && (mp.len() == prefix.len() || mp.as_bytes().get(prefix.len()) == Some(&b'/'))
        {
            results.push((mp.clone(), entry.source.clone(), entry.fstype.clone()));
        }
    }
    results
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_mountinfo() {
        assert_eq!(unescape_mountinfo("hello\\040world"), "hello world");
        assert_eq!(unescape_mountinfo("/mnt/backup"), "/mnt/backup");
        assert_eq!(unescape_mountinfo("a\\011b"), "a\tb");
    }

    #[test]
    fn test_parse_mountinfo() {
        let data = "\
22 1 259:2 /@ / rw,relatime shared:1 - btrfs /dev/nvme0n1p2 rw,compress=zstd:3,ssd,discard=async,space_cache=v2,subvolid=256,subvol=/@
23 22 259:2 /@home /home rw,relatime shared:2 - btrfs /dev/nvme0n1p2 rw,compress=zstd:3,ssd
30 22 259:1 / /boot rw,relatime shared:5 - vfat /dev/nvme0n1p1 rw,fmask=0022,dmask=0022";
        let entries = parse_mountinfo_str(data);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].mount_point, "/");
        assert_eq!(entries[0].root, "/@");
        assert_eq!(entries[0].fstype, "btrfs");
        assert_eq!(entries[0].source, "/dev/nvme0n1p2");
        assert!(entries[0].super_options.contains("compress=zstd:3"));

        assert_eq!(entries[1].mount_point, "/home");
        assert_eq!(entries[1].root, "/@home");

        assert_eq!(entries[2].mount_point, "/boot");
        assert_eq!(entries[2].fstype, "vfat");
    }

    #[test]
    fn test_find_mount() {
        let data = "\
22 1 259:2 /@ / rw,relatime shared:1 - btrfs /dev/nvme0n1p2 rw,compress=zstd:3,ssd
23 22 259:2 /@home /home rw,relatime shared:2 - btrfs /dev/nvme0n1p2 rw,compress=zstd:3";
        let entries = parse_mountinfo_str(data);

        let m = find_mount_in("/home/max", &entries).expect("/home/max should match /home");
        assert_eq!(m.mount_point, "/home");
        assert_eq!(m.root, "/@home");

        let m = find_mount_in("/", &entries).expect("/ should match /");
        assert_eq!(m.mount_point, "/");

        let m = find_mount_in("/etc/fstab", &entries).expect("/etc/fstab should match /");
        assert_eq!(m.mount_point, "/");
    }

    #[test]
    fn test_nested_mounts() {
        let data = "\
22 1 259:2 /@ / rw shared:1 - btrfs /dev/nvme0n1p2 rw
23 22 259:2 /@home /home rw shared:2 - btrfs /dev/nvme0n1p2 rw
30 22 259:1 / /boot rw shared:5 - vfat /dev/nvme0n1p1 rw
99 23 0:50 / /home/max/.gvfs rw - fuse.gvfsd-fuse gvfsd-fuse rw";
        let entries = parse_mountinfo_str(data);

        // Nested mounts under /home
        let nested: Vec<(String, String, String)> = {
            let clean = "/home";
            entries
                .iter()
                .filter(|e| {
                    &e.mount_point != clean
                        && e.mount_point.starts_with(clean)
                        && e.mount_point.as_bytes().get(clean.len()) == Some(&b'/')
                })
                .map(|e| (e.mount_point.clone(), e.source.clone(), e.fstype.clone()))
                .collect()
        };
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].0, "/home/max/.gvfs");
    }
}
