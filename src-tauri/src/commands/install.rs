//! System integration install/uninstall: binary, .desktop, polkit, pacman hook.

use super::helpers::*;
use std::fs;

// Icon embedded at compile time from the Tauri icons directory.
const ICON_128: &[u8] = include_bytes!("../../icons/128x128.png");

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct IntegrationStatus {
    pub binary: bool,
    pub desktop: bool,
    pub polkit: bool,
    pub pacman_hook: bool,
    pub binary_path: String,
}

/// Check which integration components are currently installed.
#[tauri::command]
pub async fn get_integration_status() -> Result<IntegrationStatus, String> {
    tokio::task::spawn_blocking(|| {
        Ok(IntegrationStatus {
            binary: std::path::Path::new("/usr/local/bin/arclight").exists(),
            desktop: std::path::Path::new("/usr/share/applications/arclight.desktop").exists(),
            polkit: std::path::Path::new("/etc/polkit-1/rules.d/50-arclight.rules").exists(),
            pacman_hook: std::path::Path::new("/etc/pacman.d/hooks/00-arclight-pre.hook").exists(),
            binary_path: "/usr/local/bin/arclight".to_string(),
        })
    })
    .await
    .map_err(|e| format!("Status-Thread panicked: {}", e))?
}

/// Install all system integration components (requires polkit escalation).
#[tauri::command]
pub async fn install_system_integration() -> Result<String, String> {
    tokio::task::spawn_blocking(install_integration_sync)
        .await
        .map_err(|e| format!("Install-Thread panicked: {}", e))?
}

fn install_integration_sync() -> Result<String, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Binary-Pfad: {}", e))?
        .to_string_lossy()
        .to_string();

    // Write icon to tmp (unprivileged) for binary-safe copy
    let tmp_icon = "/tmp/arclight-install.png";
    fs::write(tmp_icon, ICON_128).map_err(|e| format!("Icon schreiben: {}", e))?;

    // ── Single privileged batch for all file operations ──
    let desktop = "[Desktop Entry]
Name=arclight
GenericName=Backup Manager
Comment=Btrfs System Backup mit Snapper und rsync
Exec=/usr/local/bin/arclight
Icon=arclight
Terminal=false
Type=Application
Categories=System;Utility;
Keywords=backup;btrfs;snapper;sync;
StartupWMClass=arclight
";

    let polkit = r#"// arclight — wheel-Mitglieder duerfen arclight ohne Passwort ausfuehren
polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.policykit.exec" &&
        action.lookup("program") == "/usr/local/bin/arclight" &&
        subject.isInGroup("wheel")) {
        return polkit.Result.YES;
    }
});
"#;

    let hook = "# arclight — automatischer Snapper-Snapshot vor jedem Pacman-Update
[Trigger]
Operation = Upgrade
Operation = Install
Operation = Remove
Type = Package
Target = *

[Action]
Description = arclight: Erstelle Pre-Update Snapshot...
When = PreTransaction
Exec = /usr/bin/snapper -c root create --type=pre --cleanup-algorithm=number --print-number --description=\"pacman update\"
Depends On = pacman
";

    let ops = vec![
        // Binary
        FileOp::Copy {
            src: exe,
            dst: "/usr/local/bin/arclight".into(),
        },
        FileOp::Chmod {
            path: "/usr/local/bin/arclight".into(),
            mode: 0o755,
        },
        // Icon
        FileOp::Mkdir {
            path: "/usr/share/icons/hicolor/128x128/apps".into(),
        },
        FileOp::Copy {
            src: tmp_icon.into(),
            dst: "/usr/share/icons/hicolor/128x128/apps/arclight.png".into(),
        },
        // .desktop
        FileOp::Write {
            path: "/usr/share/applications/arclight.desktop".into(),
            content: desktop.into(),
        },
        // Polkit
        FileOp::Mkdir {
            path: "/etc/polkit-1/rules.d".into(),
        },
        FileOp::Write {
            path: "/etc/polkit-1/rules.d/50-arclight.rules".into(),
            content: polkit.into(),
        },
        // Pacman hook
        FileOp::Mkdir {
            path: "/etc/pacman.d/hooks".into(),
        },
        FileOp::Write {
            path: "/etc/pacman.d/hooks/00-arclight-pre.hook".into(),
            content: hook.into(),
        },
    ];

    run_file_ops_batch(&ops).map_err(|e| format!("Installation fehlgeschlagen: {}", e))?;
    let _ = fs::remove_file(tmp_icon);

    // External tool updates (cannot be batched as file ops)
    let _ = run_privileged("gtk-update-icon-cache", &["/usr/share/icons/hicolor"]);
    let _ = run_privileged("update-desktop-database", &["/usr/share/applications"]);

    Ok("✓ Binary → /usr/local/bin/arclight\n\
        ✓ Icon → /usr/share/icons/hicolor/128x128/apps/arclight.png\n\
        ✓ .desktop → /usr/share/applications/arclight.desktop\n\
        ✓ Polkit → /etc/polkit-1/rules.d/50-arclight.rules\n\
        ✓ Pacman-Hook → /etc/pacman.d/hooks/00-arclight-pre.hook"
        .to_string())
}

/// Remove all system integration components.
#[tauri::command]
pub async fn uninstall_system_integration() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        let ops: Vec<FileOp> = [
            "/usr/local/bin/arclight",
            "/usr/share/applications/arclight.desktop",
            "/usr/share/icons/hicolor/128x128/apps/arclight.png",
            "/etc/polkit-1/rules.d/50-arclight.rules",
            "/etc/pacman.d/hooks/00-arclight-pre.hook",
        ]
        .iter()
        .filter(|p| std::path::Path::new(p).exists())
        .map(|p| FileOp::Delete {
            path: (*p).to_string(),
        })
        .collect();

        if !ops.is_empty() {
            run_file_ops_batch(&ops)
                .map_err(|e| format!("Deinstallation fehlgeschlagen: {}", e))?;
        }

        let _ = run_privileged("gtk-update-icon-cache", &["/usr/share/icons/hicolor"]);
        let _ = run_privileged("update-desktop-database", &["/usr/share/applications"]);
        Ok("✓ Alle Komponenten entfernt".to_string())
    })
    .await
    .map_err(|e| format!("Uninstall-Thread panicked: {}", e))?
}
