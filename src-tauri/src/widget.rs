//! Widget window management — KWin integration for Wayland.
//!
//! On Wayland the compositor ignores `Window.position()`, so we use KWin
//! scripting via qdbus to save/restore the desktop widget position.

use std::path::PathBuf;
use tauri::Manager;

pub(crate) fn widget_pos_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("com.arclight.app");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("widget-position.json")
}

pub(crate) fn save_widget_pos(_w: &tauri::WebviewWindow) {
    // On Wayland, outer_position() always returns (0,0).
    // Use a KWin script to get the real position.
    let script = r#"
var clients = workspace.windowList();
for (var i = 0; i < clients.length; i++) {
  var c = clients[i];
  if (c.caption === "Arclight Widget") {
    console.log("WIDGET_POS:" + c.frameGeometry.x + "," + c.frameGeometry.y + "," + c.frameGeometry.width + "," + c.frameGeometry.height);
  }
}
"#;
    let tmp = "/tmp/arclight_widget_pos.js";
    if std::fs::write(tmp, script).is_err() {
        return;
    }

    // Load and run the script
    let load_out = std::process::Command::new("qdbus")
        .args([
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting.loadScript",
            tmp,
            "",
        ])
        .output();
    let script_id = match load_out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(_) => return,
    };
    let script_path = format!("/Scripting/Script{}", script_id);
    let _ = std::process::Command::new("qdbus")
        .args(["org.kde.KWin", &script_path, "org.kde.kwin.Script.run"])
        .output();

    // Give KWin a moment, then read from journal
    std::thread::sleep(std::time::Duration::from_millis(200));
    if let Ok(journal) = std::process::Command::new("journalctl")
        .args([
            "--user",
            "-u",
            "plasma-kwin_wayland",
            "-n",
            "20",
            "--no-pager",
            "-o",
            "cat",
        ])
        .output()
    {
        let out = String::from_utf8_lossy(&journal.stdout);
        for line in out.lines().rev() {
            if let Some(data) = line.strip_prefix("Arclight Widget: x=") {
                // Parse "x=852 y=0 w=228 h=506"
                let parts: Vec<&str> = data.split_whitespace().collect();
                if parts.len() == 4 {
                    let x = parts[0].parse::<f64>().unwrap_or(0.0);
                    let y = parts[1]
                        .strip_prefix("y=")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let w = parts[2]
                        .strip_prefix("w=")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(320.0);
                    let h = parts[3]
                        .strip_prefix("h=")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(420.0);
                    let json = format!(r#"{{"x":{},"y":{},"width":{},"height":{}}}"#, x, y, w, h);
                    let _ = std::fs::write(widget_pos_path(), &json);
                    log::info!("Widget position saved (KWin): {}", json);
                    return;
                }
            }
            if let Some(data) = line.strip_prefix("WIDGET_POS:") {
                let parts: Vec<&str> = data.split(',').collect();
                if parts.len() == 4 {
                    let json = format!(
                        r#"{{"x":{},"y":{},"width":{},"height":{}}}"#,
                        parts[0], parts[1], parts[2], parts[3]
                    );
                    let _ = std::fs::write(widget_pos_path(), &json);
                    log::info!("Widget position saved (KWin): {}", json);
                    return;
                }
            }
        }
    }
    log::warn!("Could not read widget position from KWin");
}

pub(crate) fn load_widget_pos() -> Option<(f64, f64, f64, f64)> {
    let data = std::fs::read_to_string(widget_pos_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    Some((
        v["x"].as_f64()?,
        v["y"].as_f64()?,
        v["width"].as_f64().unwrap_or(320.0),
        v["height"].as_f64().unwrap_or(420.0),
    ))
}

pub(crate) fn toggle_widget(app_handle: &tauri::AppHandle) {
    if let Some(w) = app_handle.get_webview_window("widget") {
        if w.is_visible().unwrap_or_default() {
            save_widget_pos(&w);
            let _ = w.hide();
        } else {
            let _ = w.show();
            // Restore position on Wayland via KWin
            if let Some((x, y, w, h)) = load_widget_pos() {
                move_widget_kwin(x, y, w, h);
            }
        }
    } else {
        // Create the widget window
        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            "widget",
            tauri::WebviewUrl::App("/widget".into()),
        )
        .title("Arclight Widget")
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .always_on_bottom(true)
        .skip_taskbar(true);

        if let Some((x, y, w, h)) = load_widget_pos() {
            builder = builder.position(x, y).inner_size(w, h);
        } else {
            builder = builder.inner_size(320.0, 420.0);
        }

        match builder.build() {
            Ok(_w) => {
                log::info!("Widget window created");
                // On Wayland, .position() is ignored by the compositor.
                // Use KWin scripting to move the window to the saved position.
                if let Some((x, y, w, h)) = load_widget_pos() {
                    move_widget_kwin(x, y, w, h);
                }
            }
            Err(e) => {
                log::error!("Failed to create widget window: {}", e);
            }
        }
    }
}

// CAST-SAFETY: pixel coordinates from window geometry; truncation is acceptable
#[allow(clippy::cast_possible_truncation)]
fn move_widget_kwin(x: f64, y: f64, w: f64, h: f64) {
    std::thread::spawn(move || {
        // Give the window a moment to appear
        std::thread::sleep(std::time::Duration::from_millis(500));
        let script = format!(
            r#"var clients = workspace.windowList();
for (var i = 0; i < clients.length; i++) {{
  var c = clients[i];
  if (c.caption === "Arclight Widget") {{
    c.frameGeometry = {{x: {x}, y: {y}, width: {w}, height: {h}}};
    console.log("Widget moved to {x},{y}");
  }}
}}"#,
            x = x as i64,
            y = y as i64,
            w = w as i64,
            h = h as i64,
        );
        let tmp = "/tmp/arclight_move_widget.js";
        if std::fs::write(tmp, &script).is_err() {
            return;
        }
        // Unload any previously loaded version
        let _ = std::process::Command::new("qdbus")
            .args([
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.unloadScript",
                tmp,
            ])
            .output();
        let load_out = std::process::Command::new("qdbus")
            .args([
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                tmp,
                "",
            ])
            .output();
        if let Ok(o) = load_out {
            let id = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if id != "-1" {
                let path = format!("/Scripting/Script{}", id);
                let _ = std::process::Command::new("qdbus")
                    .args(["org.kde.KWin", &path, "org.kde.kwin.Script.run"])
                    .output();
                log::info!("Widget moved via KWin script {}", id);
            }
        }
    });
}
