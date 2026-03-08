use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::command;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub icon_path: Option<String>,
    pub terminal: bool,
}

fn app_directories() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
    }
    dirs
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn icon_search_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/icons/hicolor"),
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
        PathBuf::from("/var/lib/flatpak/exports/share/icons/hicolor"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".local/share/icons/hicolor"));
        roots.push(home.join(".icons"));
        roots.push(home.join(".local/share/flatpak/exports/share/icons/hicolor"));
    }
    roots
}

fn candidate_icon_names(icon_name: &str) -> Vec<String> {
    let trimmed = icon_name.trim();
    let mut names = vec![trimmed.to_owned()];
    if let Some(file_name) = Path::new(trimmed)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
    {
        if file_name != trimmed {
            names.push(file_name.to_owned());
        }
    }
    if let Some(stem) = Path::new(trimmed)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
    {
        if !names.iter().any(|item| item == stem) {
            names.push(stem.to_owned());
        }
    }
    names
}

fn resolve_icon_path(icon_name: &str) -> Option<String> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    let direct = PathBuf::from(trimmed);
    if direct.is_file() {
        return Some(direct.to_string_lossy().into_owned());
    }

    let names = candidate_icon_names(trimmed);
    let exts = ["png", "svg", "xpm", "ico"];

    for root in icon_search_roots() {
        for name in &names {
            for ext in &exts {
                let direct_file = root.join(format!("{name}.{ext}"));
                if direct_file.is_file() {
                    return Some(direct_file.to_string_lossy().into_owned());
                }
                let scalable = root.join("scalable/apps").join(format!("{name}.{ext}"));
                if scalable.is_file() {
                    return Some(scalable.to_string_lossy().into_owned());
                }
                let apps_128 = root.join("128x128/apps").join(format!("{name}.{ext}"));
                if apps_128.is_file() {
                    return Some(apps_128.to_string_lossy().into_owned());
                }
                let apps_64 = root.join("64x64/apps").join(format!("{name}.{ext}"));
                if apps_64.is_file() {
                    return Some(apps_64.to_string_lossy().into_owned());
                }
                let apps_48 = root.join("48x48/apps").join(format!("{name}.{ext}"));
                if apps_48.is_file() {
                    return Some(apps_48.to_string_lossy().into_owned());
                }
                let apps_32 = root.join("32x32/apps").join(format!("{name}.{ext}"));
                if apps_32.is_file() {
                    return Some(apps_32.to_string_lossy().into_owned());
                }
            }
        }
    }

    None
}

fn parse_desktop_file(path: &Path) -> Option<DesktopApp> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut in_entry = false;
    let mut is_application = false;
    let mut hidden = false;
    let mut no_display = false;
    let mut terminal = false;
    let mut name: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon: Option<String> = None;

    for line in reader.lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "Type" => is_application = value == "Application",
            "Name" if name.is_none() => name = Some(value.to_owned()),
            "Exec" if exec.is_none() => exec = Some(value.to_owned()),
            "Icon" if icon.is_none() => icon = Some(value.to_owned()),
            "NoDisplay" => no_display = parse_bool(value),
            "Hidden" => hidden = parse_bool(value),
            "Terminal" => terminal = parse_bool(value),
            _ => {}
        }
    }

    if !is_application || hidden || no_display {
        return None;
    }

    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let icon_path = icon.as_deref().and_then(resolve_icon_path);
    Some(DesktopApp {
        id: file_name,
        name: name?,
        exec: exec?,
        icon,
        icon_path,
        terminal,
    })
}

fn discover_desktop_apps() -> Vec<DesktopApp> {
    let mut apps = BTreeMap::<String, DesktopApp>::new();

    for dir in app_directories() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_desktop = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("desktop"));
            if !is_desktop {
                continue;
            }
            if let Some(app) = parse_desktop_file(&path) {
                apps.entry(app.id.clone()).or_insert(app);
            }
        }
    }

    let mut values: Vec<_> = apps.into_values().collect();
    values.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    values
}

fn sanitize_exec(exec: &str) -> String {
    let mut cleaned = String::new();
    let mut chars = exec.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.peek().copied() {
                Some('%') => {
                    cleaned.push('%');
                    let _ = chars.next();
                }
                Some(_) => {
                    let _ = chars.next();
                }
                None => {}
            }
            continue;
        }
        cleaned.push(ch);
    }

    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn try_spawn(command: &str, args: &[&str]) -> Result<(), String> {
    Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{command}: {e}"))
}

#[command]
pub fn list_desktop_apps() -> Result<Vec<DesktopApp>, String> {
    Ok(discover_desktop_apps())
}

#[command]
pub fn launch_desktop_app(app_id: String) -> Result<String, String> {
    let apps = discover_desktop_apps();
    let app = apps
        .into_iter()
        .find(|item| item.id == app_id)
        .ok_or_else(|| format!("Desktop app not found: {app_id}"))?;

    if try_spawn("gtk-launch", &[&app.id]).is_ok() {
        return Ok(format!("{} launched.", app.name));
    }

    let exec = sanitize_exec(&app.exec);
    if exec.is_empty() {
        return Err(format!("No launch command available for {}", app.name));
    }

    try_spawn("sh", &["-lc", &exec]).map(|_| format!("{} launched.", app.name))
}