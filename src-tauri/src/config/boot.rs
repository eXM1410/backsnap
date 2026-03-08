//! Bootloader detection (systemd-boot, grub).

use super::types::BootloaderType;
use crate::util::safe_cmd;

/// Detect which bootloader is installed.
pub fn detect_bootloader() -> BootloaderType {
    // 1. bootctl status — fastest and most reliable
    if let Some(o) = safe_cmd("bootctl", &["status"]) {
        let out = String::from_utf8_lossy(&o.stdout);
        if out.contains("systemd-boot") || out.contains("Boot Loader Specification") {
            return BootloaderType::SystemdBoot;
        }
    }

    // 2. Filesystem checks
    if std::path::Path::new("/boot/EFI/systemd/systemd-bootx64.efi").exists()
        || std::path::Path::new("/boot/efi/EFI/systemd/systemd-bootx64.efi").exists()
        || std::path::Path::new("/efi/EFI/systemd/systemd-bootx64.efi").exists()
        || std::path::Path::new("/boot/loader/loader.conf").exists()
    {
        return BootloaderType::SystemdBoot;
    }
    if std::path::Path::new("/boot/grub/grub.cfg").exists()
        || std::path::Path::new("/boot/grub2/grub.cfg").exists()
    {
        return BootloaderType::Grub;
    }

    BootloaderType::SystemdBoot
}
