//! Systemd timer commands: install, uninstall, status, enable/disable.

use super::helpers::*;
use crate::config;
use std::process::Command;

// ─── Timer Config ─────────────────────────────────────────────

#[tauri::command]
pub async fn get_timer_config() -> Result<TimerConfig, String> {
    tokio::task::spawn_blocking(|| Ok(get_timer_config_sync()))
        .await
        .map_err(|e| format!("Timer-Config thread panicked: {}", e))?
}

pub(super) fn get_timer_config_sync() -> TimerConfig {
    let c = cfg();

    let cat_check = run_cmd("systemctl", &["cat", &c.sync.timer_unit]);
    if !cat_check.success {
        return TimerConfig {
            enabled: false,
            calendar: String::new(),
            randomized_delay: "0".to_string(),
            last_trigger: None,
            service_result: None,
        };
    }

    let active = run_cmd("systemctl", &["is-active", &c.sync.timer_unit]);
    let enabled = active.stdout.trim() == "active";

    let props = run_cmd(
        "systemctl",
        &[
            "show",
            &c.sync.timer_unit,
            "--property=TimersCalendar,RandomizedDelayUSec,LastTriggerUSec",
            "--no-pager",
        ],
    );

    let mut calendar = "daily".to_string();
    let mut delay = "1h".to_string();
    let mut last_trigger = None;

    for line in props.stdout.lines() {
        if let Some(val) = line.strip_prefix("TimersCalendar=") {
            calendar = val.split_whitespace().last().unwrap_or("daily").to_string();
        } else if let Some(val) = line.strip_prefix("RandomizedDelayUSec=") {
            delay = val.to_string();
        } else if let Some(val) = line.strip_prefix("LastTriggerUSec=") {
            if !val.is_empty() {
                last_trigger = Some(val.to_string());
            }
        }
    }

    let svc = run_cmd(
        "systemctl",
        &[
            "show",
            &c.sync.service_unit,
            "--property=Result",
            "--no-pager",
        ],
    );
    let service_result = svc
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("Result="))
        .map(std::string::ToString::to_string);

    TimerConfig {
        enabled,
        calendar,
        randomized_delay: delay,
        last_trigger,
        service_result,
    }
}

// ─── Timer Enable/Disable ─────────────────────────────────────

#[tauri::command]
pub async fn set_timer_enabled(enabled: bool) -> Result<CommandResult, String> {
    tokio::task::spawn_blocking(move || {
        let c = cfg();
        let action = if enabled { "enable" } else { "disable" };
        Ok(run_privileged(
            "systemctl",
            &[action, "--now", &c.sync.timer_unit],
        ))
    })
    .await
    .map_err(|e| format!("Timer-Thread panicked: {}", e))?
}

// ─── Timer Install ────────────────────────────────────────────

fn validate_timer_value(val: &str) -> Result<(), String> {
    if val.is_empty() || val.len() > 128 {
        return Err("Timer-Wert ungültig: leer oder zu lang".to_string());
    }
    let forbidden = [
        '`', '$', '\\', '|', ';', '&', '<', '>', '\n', '\r', '\0', '\'', '"',
    ];
    if val.chars().any(|c| forbidden.contains(&c)) {
        return Err("Timer-Wert enthält ungültige Zeichen".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn install_timer(calendar: String, delay: String) -> Result<CommandResult, String> {
    validate_timer_value(&calendar)?;
    validate_timer_value(&delay)?;

    tokio::task::spawn_blocking(move || {
        let c = cfg();

        let exe = std::env::current_exe()
            .map_err(|e| format!("Binary-Pfad nicht ermittelt: {}", e))?
            .to_string_lossy()
            .to_string();

        let config_path = config::config_path().to_string_lossy().into_owned();
        validate_safe_path(&exe, "Binary-Pfad")?;
        validate_safe_path(&config_path, "Config-Pfad")?;

    let uid = Command::new("id")
        .args(["-u"])
        .output().map_or_else(|_| "1000".to_string(), |o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let service_content = format!(
        "[Unit]\n\
         Description=backsnap System Sync\n\
         After=local-fs.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=\"{exe}\" --sync --config \"{config}\"\n\
         Nice=19\n\
         IOSchedulingClass=idle\n\
         # XDG_RUNTIME_DIR is needed for some privilege helpers (pkexec/polkit).\n\
         # No DISPLAY/XAUTHORITY/WAYLAND_DISPLAY: --sync is fully headless.\n\
         Environment=\"XDG_RUNTIME_DIR=/run/user/{uid}\"\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        exe = exe,
        config = config_path,
        uid = uid,
    );

    let timer_content = format!(
        "[Unit]\n\
         Description=backsnap Sync Timer\n\
         \n\
         [Timer]\n\
         OnCalendar={calendar}\n\
         RandomizedDelaySec={delay}\n\
         Persistent=true\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        calendar = calendar,
        delay = delay,
    );

    let rapl_content =
        "[Unit]\n\
         Description=backsnap RAPL energy permissions\n\
         After=local-fs.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/bin/sh -c 'for d in /sys/class/powercap/*rapl*; do [ -d \"$d\" ] && chmod -R a+rX \"$d\" || true; done'\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n";

    let svc_path = format!("/etc/systemd/system/{}", c.sync.service_unit);
    let tmr_path = format!("/etc/systemd/system/{}", c.sync.timer_unit);
    let rapl_path = "/etc/systemd/system/backsnap-rapl-perms.service".to_string();

    // Single privileged batch for all file writes
    let ops = vec![
        FileOp::Write { path: svc_path, content: service_content },
        FileOp::Write { path: tmr_path, content: timer_content },
        FileOp::Write { path: rapl_path, content: rapl_content.to_string() },
    ];
    run_file_ops_batch(&ops)
        .map_err(|e| format!("Timer-Dateien installieren: {}", e))?;

    let _ = run_privileged("systemctl", &["enable", "backsnap-rapl-perms.service"]);
    let _ = run_privileged("systemctl", &["start", "backsnap-rapl-perms.service"]);

    let r = run_privileged("systemctl", &["daemon-reload"]);
    if !r.success {
        return Err(format!("daemon-reload: {}", r.stderr));
    }

    let r = run_privileged("systemctl", &["enable", "--now", &c.sync.timer_unit]);
    if !r.success {
        return Err(format!("Timer aktivieren: {}", r.stderr));
    }

    Ok(CommandResult {
        success: true,
        stdout: format!(
            "Timer {} installiert und aktiviert.\nIntervall: {}, Verzögerung: {}\nBinary: {}\nConfig: {}",
            c.sync.timer_unit, calendar, delay, exe, config_path
        ),
        stderr: String::new(),
        exit_code: 0,
    })
    }) // end spawn_blocking
    .await
    .map_err(|e| format!("Timer-Thread panicked: {}", e))?
}

// ─── Timer Uninstall ──────────────────────────────────────────

#[tauri::command]
pub async fn uninstall_timer() -> Result<CommandResult, String> {
    tokio::task::spawn_blocking(|| {
        let c = cfg();

        let _ = run_privileged("systemctl", &["disable", "--now", &c.sync.timer_unit]);
        let _ = run_privileged("systemctl", &["stop", &c.sync.service_unit]);

        let svc_path = format!("/etc/systemd/system/{}", c.sync.service_unit);
        let tmr_path = format!("/etc/systemd/system/{}", c.sync.timer_unit);

        let ops = vec![
            FileOp::Delete { path: svc_path },
            FileOp::Delete { path: tmr_path },
            FileOp::Delete {
                path: "/etc/systemd/system/backsnap-rapl-perms.service".into(),
            },
        ];

        let _ = run_privileged(
            "systemctl",
            &["disable", "--now", "backsnap-rapl-perms.service"],
        );
        let _ = run_file_ops_batch(&ops);
        let _ = run_privileged("systemctl", &["daemon-reload"]);

        Ok(CommandResult {
            success: true,
            stdout: format!("Timer {} deinstalliert", c.sync.timer_unit),
            stderr: String::new(),
            exit_code: 0,
        })
    })
    .await
    .map_err(|e| format!("Timer-Thread panicked: {}", e))?
}
