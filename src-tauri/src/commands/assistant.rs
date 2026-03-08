//! Jarvis — high-level Tauri bindings and voice-flow orchestration.

mod audio;
mod core;
mod listener;
mod voice_state;

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tauri::command;
use tauri::{AppHandle, Emitter, Manager};

use self::audio::{
    dec_speaking_guard, inc_speaking_guard, kill_active_tts, pick_entrance_path,
    play_cached_ack, play_entrance_audio, tts_speak, wait_for_entrance_playback,
};
use self::core::execute_voice_command;
use self::listener::ListenerEvent;
use self::voice_state::{
    begin_clap_session, begin_voice_command, begin_wake_acknowledgement,
    finish_voice_command as finish_voice_state_command, finish_wake_acknowledgement,
    reset as reset_voice_state,
};

pub use self::audio::{
    ensure_orpheus_tts_running, jarvis_listen_sync, jarvis_speak_sync, transcribe_audio_sync,
    tts_generate_sync,
};

pub use self::audio::ensure_stt_worker as warmup_stt_worker;

const CLAP_ARM_DELAY_MS: u64 = 1_100;
const ENTRANCE_RESPONSE_WAIT_MS: u64 = 2_400;
const AUDIO_HANDOFF_DELAY_MS: u64 = 140;

/// Detect when Whisper captures the cached ack WAV itself via AEC leakage.
/// These are the exact phrases Orpheus generates for the "Yes, Sir?" ack.
fn is_ack_leakage(text: &str) -> bool {
    let lower = text.to_lowercase();
    let lower = lower.trim_end_matches(['.', '?', '!', ',']);
    matches!(
        lower,
        "yes" | "yes sir" | "yes, sir" | "ja" | "ja sir" | "ja, sir"
            | "yes sir?" | "ja sir?"
    )
}

/// Global app handle — set once in lib.rs setup, used by clap/wake threads.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Global on/off switch for always-listening Jarvis wake detection.
static LISTENER_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantResponse {
    pub text: String,
    pub actions: Vec<ActionResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub action: String,
    pub success: bool,
    pub message: String,
}

/// Used by `intent` fast-parser to describe actions to execute.
#[derive(Debug, Clone)]
pub struct KeywordAction {
    pub action: String,
    pub params: serde_json::Value,
}

pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

fn focus_main_window(handle: &AppHandle) {
    if let Some(w) = handle.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn spawn_focus_fallbacks() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(200));

        let _ = Command::new("wmctrl")
            .args(["-r", "Arclight", "-b", "add,above"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("wmctrl")
            .args(["-a", "Arclight"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    });
}

fn show_and_navigate_assistant() {
    if let Some(handle) = APP_HANDLE.get() {
        focus_main_window(handle);
        let _ = handle.emit("navigate-assistant", ());

        // Delayed fullscreen + always-on-top so the UI render isn't blocked
        let handle2 = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(180));
            if let Some(w) = handle2.get_webview_window("main") {
                if !w.is_fullscreen().unwrap_or(true) {
                    let _ = w.set_fullscreen(true);
                }
                let _ = w.set_always_on_top(true);
                let _ = w.set_focus();
            }
        });

        spawn_focus_fallbacks();
    }
}

#[command]
pub async fn assistant_chat(history: Vec<ChatMessage>) -> AssistantResponse {
    tokio::task::spawn_blocking(move || assistant_chat_sync(history))
        .await
        .unwrap_or_else(|e| AssistantResponse {
            text: format!("⚠️ Task error: {e}"),
            actions: vec![],
        })
}

pub fn assistant_chat_sync(history: Vec<ChatMessage>) -> AssistantResponse {
    core::assistant_chat_sync(history)
}

#[command]
pub async fn assistant_status() -> (bool, String) {
    tokio::task::spawn_blocking(assistant_status_sync)
        .await
        .unwrap_or((false, "Task error".into()))
}

pub fn assistant_status_sync() -> (bool, String) {
    core::assistant_status_sync()
}

#[command]
pub async fn jarvis_speak(text: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || jarvis_speak_sync(text))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

#[command]
pub async fn jarvis_listen() -> Result<String, String> {
    tokio::task::spawn_blocking(jarvis_listen_sync)
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

#[command]
pub fn jarvis_listener_enabled() -> bool {
    LISTENER_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

#[command]
pub fn jarvis_set_listener_enabled(enabled: bool) -> bool {
    LISTENER_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);

    if !enabled {
        reset_voice_state("listener disabled");
        audio::stop_tracked_audio();
        let _ = Command::new("pkill")
            .args(["-f", "openwake_listener.py"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    enabled
}

pub fn spawn_clap_listener() {
    audio::clear_speaking_on_startup();
    std::thread::Builder::new()
        .name("jarvis-listener".into())
        .spawn(|| listener::listener_loop(jarvis_listener_enabled, handle_listener_event))
        .expect("Failed to spawn Jarvis listener thread");
}

fn speak_voice_response(text: String, log_prefix: &str) {
    if text.is_empty() {
        return;
    }

    if let Err(e) = tts_speak(&text, true) {
        log::warn!("[{log_prefix}] TTS failed for command response: {e}");
    }
}

fn capture_followup_command(log_prefix: &'static str, empty_reason: &'static str) {
    std::thread::sleep(std::time::Duration::from_millis(AUDIO_HANDOFF_DELAY_MS));

    match jarvis_listen_sync() {
        Ok(command) => {
            let command = command.trim().to_string();
            if command.is_empty() {
                reset_voice_state(empty_reason);
            } else {
                log::info!("[{log_prefix}] Follow-up command captured: {command}");
                spawn_voice_command(command, log_prefix);
            }
        }
        Err(e) => {
            log::info!("[{log_prefix}] No follow-up command captured: {e}");
            reset_voice_state(empty_reason);
        }
    }
}

fn spawn_clap_session() {
    if !begin_clap_session() {
        return;
    }

    std::thread::spawn(|| {
        show_and_navigate_assistant();
        std::thread::spawn(|| {
            let _ = super::lighting::lighting_master_power(true);
        });

        let entrance_path = pick_entrance_path();
        if let Err(e) = play_entrance_audio(&entrance_path) {
            log::warn!("[jarvis-clap] Entrance audio failed: {e}");
        }

        let _ = wait_for_entrance_playback(CLAP_ARM_DELAY_MS, false);
        voice_state::arm_command_window_after_clap();
        capture_followup_command("jarvis-clap", "clap follow-up capture failed");
    });
}

fn spawn_wake_acknowledgement() {
    let Some(effects) = begin_wake_acknowledgement() else {
        return;
    };

    std::thread::spawn(move || {
        // Hold speaking flag so the *listener* (raw mic, no AEC) ignores
        // our TTS output.  The STT capture uses jarvis_clean (AEC+NS+AGC)
        // which cancels our own audio, so it can record in parallel.
        inc_speaking_guard();
        show_and_navigate_assistant();
        let interrupted = if effects.stop_entrance {
            wait_for_entrance_playback(ENTRANCE_RESPONSE_WAIT_MS, true)
        } else {
            false
        };
        if interrupted {
            std::thread::sleep(std::time::Duration::from_millis(AUDIO_HANDOFF_DELAY_MS));
        }

        // Start STT capture BEFORE TTS — jarvis_clean's AEC filters out
        // our own "Yes, Sir?" so only the user's voice is captured.
        let listen_handle = std::thread::Builder::new()
            .name("jarvis-wake-listen".into())
            .spawn(jarvis_listen_sync)
            .expect("spawn wake-listen thread");

        // Brief settle time for pw-cat to connect before audio plays
        std::thread::sleep(std::time::Duration::from_millis(60));

        // Play cached acknowledgement while already recording (~50ms vs ~800ms TTS)
        if let Err(e) = play_cached_ack(true) {
            log::warn!("[jarvis-wake] Cached ack failed, falling back to TTS: {e}");
            if let Err(e2) = tts_speak("Yes, Sir?", true) {
                log::warn!("[jarvis-wake] TTS fallback also failed: {e2}");
            }
        }
        finish_wake_acknowledgement();
        dec_speaking_guard();

        // Collect the STT result (user may speak during or after TTS)
        match listen_handle.join() {
            Ok(Ok(command)) => {
                let command = command.trim().to_string();
                if command.is_empty() || is_ack_leakage(&command) {
                    if !command.is_empty() {
                        log::info!("[jarvis-followup] Discarded AEC-leaked ack: {command}");
                    }
                    reset_voice_state("wake follow-up capture empty");
                } else {
                    log::info!("[jarvis-followup] Follow-up command captured: {command}");
                    spawn_voice_command(command, "jarvis-followup");
                }
            }
            Ok(Err(e)) => {
                log::info!("[jarvis-followup] No follow-up command captured: {e}");
                reset_voice_state("wake follow-up capture failed");
            }
            Err(_) => {
                reset_voice_state("wake listen thread panicked");
            }
        }
    });
}

fn spawn_voice_command(command: String, log_prefix: &'static str) {
    let Some(effects) = begin_voice_command() else {
        return;
    };

    if effects.stop_tts {
        kill_active_tts();
    }

    std::thread::spawn(move || {
        // Hold speaking flag for the ENTIRE command flow (entrance wait +
        // acknowledgement TTS + command TTS) so Python never hears us.
        inc_speaking_guard();
        let cmd_start = std::time::Instant::now();
        show_and_navigate_assistant();

        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let prepare_prefix = log_prefix;
        std::thread::spawn(move || {
            let result = execute_voice_command(command, prepare_prefix);
            let _ = result_tx.send(result);
        });

        let interrupted = if effects.stop_entrance {
            wait_for_entrance_playback(ENTRANCE_RESPONSE_WAIT_MS, true)
        } else {
            false
        };

        if interrupted {
            std::thread::sleep(std::time::Duration::from_millis(AUDIO_HANDOFF_DELAY_MS));
        }

        let (mut resp, worker) = match result_rx.recv() {
            Ok(result) => result,
            Err(_) => (
                AssistantResponse {
                    text: "⚠️ Voice command preparation failed".into(),
                    actions: vec![],
                },
                None,
            ),
        };

        speak_voice_response(resp.text, log_prefix);

        if let Some(worker) = worker {
            resp.actions = match worker.join() {
                Ok(results) => results,
                Err(_) => vec![ActionResult {
                    action: "voice_command".into(),
                    success: false,
                    message: "Voice command worker panicked".into(),
                }],
            };

            for result in resp.actions.iter().filter(|result| !result.success) {
                log::warn!("[{log_prefix}] Action failed: {}", result.message);
            }
        }

        log::info!(
            "[{log_prefix}] Command complete ({}ms total)",
            cmd_start.elapsed().as_millis()
        );
        finish_voice_state_command("voice command finished");
        dec_speaking_guard();
    });
}

fn handle_listener_event(event: ListenerEvent) {
    match event {
        ListenerEvent::Clap => {
            log::info!("[jarvis-listener] Clap detected");
            spawn_clap_session();
        }
        ListenerEvent::Wake => {
            log::info!("[jarvis-listener] Wake word detected");
            spawn_wake_acknowledgement();
        }
        ListenerEvent::Telemetry(telem) => {
            if let Some(handle) = APP_HANDLE.get() {
                let _ = handle.emit("jarvis-audio-telemetry", telem);
            }
        }
    }
}
