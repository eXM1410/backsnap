use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::process::{Command, Stdio};

const LISTENER_SCRIPT: &str = "/home/max/.local/share/jarvis/openwake_listener.py";
const LISTENER_PYTHON: &str = "/home/max/.local/share/jarvis-stt-gpu/bin/python3";
const LISTENER_RESTART_DELAY_MS: u64 = 1_000;

#[derive(Debug)]
pub(crate) enum ListenerEvent {
    Clap,
    Wake,
    Telemetry(AudioTelemetry),
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AudioTelemetry {
    pub rms: u32,
    pub peak: u32,
    pub wake: f32,
    pub state: String,
}

#[derive(Debug, Deserialize)]
struct ListenerWireEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    rms: Option<u32>,
    #[serde(default)]
    peak: Option<u32>,
    #[serde(default)]
    wake: Option<f32>,
    #[serde(default)]
    state: Option<String>,
}

fn spawn_listener_stderr_logger(stderr: std::process::ChildStderr) {
    std::thread::Builder::new()
        .name("jarvis-listener-stderr".into())
        .spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        let trimmed = line.trim();
                        if trimmed.contains("hipBLASLt on an unsupported architecture")
                            || trimmed == "return F.linear("
                        {
                            continue;
                        }
                        log::info!("[jarvis-listener][py] {trimmed}");
                    }
                    Ok(_) => {}
                    Err(err) => {
                        log::warn!("[jarvis-listener] stderr read error: {err}");
                        break;
                    }
                }
            }
        })
        .ok();
}

fn read_listener_event(line: &str) -> Option<ListenerEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let event = match serde_json::from_str::<ListenerWireEvent>(trimmed) {
        Ok(event) => event,
        Err(err) => {
            log::debug!("[jarvis-listener] Ignoring non-JSON listener line: {trimmed} ({err})");
            return None;
        }
    };

    match event.kind.as_str() {
        "ready" => {
            log::info!("[jarvis-listener] STT model loaded — listening active");
            None
        }
        "clap" => Some(ListenerEvent::Clap),
        "wake" => Some(ListenerEvent::Wake),
        "telemetry" => Some(ListenerEvent::Telemetry(AudioTelemetry {
            rms: event.rms.unwrap_or(0),
            peak: event.peak.unwrap_or(0),
            wake: event.wake.unwrap_or(0.0),
            state: event.state.unwrap_or_default(),
        })),
        other => {
            log::debug!("[jarvis-listener] Ignoring unknown listener event type: {other}");
            None
        }
    }
}

pub(crate) fn listener_loop<F, G>(mut is_enabled: G, mut handle_event: F)
where
    G: FnMut() -> bool,
    F: FnMut(ListenerEvent),
{
    log::info!("[jarvis-listener] Starting unified listener immediately");

    loop {
        if !is_enabled() {
            std::thread::sleep(std::time::Duration::from_millis(250));
            continue;
        }

        let child = Command::new(LISTENER_PYTHON)
            .arg(LISTENER_SCRIPT)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut proc) => {
                log::info!("[jarvis-listener] Listener process spawned");
                if let Some(stderr) = proc.stderr.take() {
                    spawn_listener_stderr_logger(stderr);
                }

                if let Some(stdout) = proc.stdout.take() {
                    let reader = std::io::BufReader::new(stdout);
                    for line in reader.lines() {
                        if !is_enabled() {
                            break;
                        }

                        match line {
                            Ok(line) => {
                                if let Some(event) = read_listener_event(&line) {
                                    handle_event(event);
                                }
                            }
                            Err(err) => {
                                log::warn!("[jarvis-listener] Read error: {err}");
                                break;
                            }
                        }
                    }
                }

                let _ = proc.kill();
                let _ = proc.wait();
                log::warn!("[jarvis-listener] Listener process exited — restarting");
            }
            Err(e) => {
                log::warn!("[jarvis-listener] Failed to spawn listener: {e}");
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(LISTENER_RESTART_DELAY_MS));
    }
}
