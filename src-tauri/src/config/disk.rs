//! Disk detection: enumerate btrfs partitions via lsblk.

use serde::{Deserialize, Serialize};

use crate::util::{format_size, safe_cmd};

/// A detected btrfs disk for the user to choose from.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DetectedDisk {
    pub device: String,
    pub uuid: String,
    pub label: String,
    pub size: String,
    pub model: String,
    pub mountpoint: Option<String>,
    pub is_boot: bool,
}

/// Detect all btrfs partitions on the system.
pub fn detect_btrfs_disks() -> Vec<DetectedDisk> {
    let result = safe_cmd(
        "lsblk",
        &[
            "-o",
            "NAME,UUID,LABEL,SIZE,MODEL,MOUNTPOINT,FSTYPE,TYPE",
            "-J",
            "-b",
        ],
    );

    let output = match result {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return Vec::new(),
    };

    let json: serde_json::Value = match serde_json::from_str(&output) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let boot_uuid = crate::sysfs::mount_uuid("/").unwrap_or_default();

    let mut disks = Vec::new();

    fn walk_devices(
        devices: &serde_json::Value,
        parent_model: &str,
        boot_uuid: &str,
        results: &mut Vec<DetectedDisk>,
    ) {
        if let Some(arr) = devices.as_array() {
            for dev in arr {
                let fstype = dev["fstype"].as_str().unwrap_or_default();
                let dev_type = dev["type"].as_str().unwrap_or_default();
                let uuid = dev["uuid"].as_str().unwrap_or_default();
                let model = dev["model"].as_str().unwrap_or(parent_model);
                let name = dev["name"].as_str().unwrap_or_default();

                if fstype == "btrfs" && dev_type == "part" && !uuid.is_empty() {
                    let size_bytes: u64 = dev["size"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| dev["size"].as_u64())
                        .unwrap_or_default();
                    let size = format_size(size_bytes);
                    let mountpoint = dev["mountpoint"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(std::string::ToString::to_string);
                    let label = dev["label"].as_str().unwrap_or_default().to_string();

                    results.push(DetectedDisk {
                        device: format!("/dev/{}", name),
                        uuid: uuid.to_string(),
                        label: if label.is_empty() {
                            model.to_string()
                        } else {
                            label
                        },
                        size,
                        model: model.to_string(),
                        mountpoint,
                        is_boot: uuid == boot_uuid,
                    });
                }

                if let Some(children) = dev.get("children") {
                    walk_devices(children, model, boot_uuid, results);
                }
            }
        }
    }

    if let Some(devices) = json.get("blockdevices") {
        walk_devices(devices, "", &boot_uuid, &mut disks);
    }

    disks
}
