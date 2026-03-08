//! Jarvis — voice-flow orchestration and Tauri commands.
//!
//! Two triggers (clap / wake) → one unified `handle_activation()`.
//! A single `SpeakingGuard` (RAII) owns the entire session lifecycle.
//! No refcounts, no state machine, no phase deadlines.

mod audio;
mod core;
mod listener;

use super::intent;
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tauri::command;
use tauri::{AppHandle, Emitter, Manager};

use self::audio::{
    jarvis_listen_sync_with_ignore_ms, kill_active_tts, pick_cached_ack_path, pick_entrance_path,
    play_cached_ack, play_cached_ack_file, play_entrance_audio, stop_tracked_audio, tts_speak,
    wait_for_entrance_playback, wav_duration_ms, SpeakingGuard, SESSION_ACTIVE,
};
use self::core::execute_voice_command;
use self::listener::ListenerEvent;

pub use self::audio::{
    ensure_orpheus_tts_running, jarvis_listen_sync, jarvis_speak_sync, transcribe_audio_sync,
    tts_generate_sync,
};
pub use self::audio::ensure_stt_worker as warmup_stt_worker;

// ── Types (public API, used by gateway + intent) ────────────────────

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

#[derive(Debug, Clone)]
pub struct KeywordAction {
    pub action: String,
    pub params: serde_json::Value,
}

// ── Globals ─────────────────────────────────────────────────────────

static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static LISTENER_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatStreamMessage {
    role: String,
    content: String,
    actions: Option<Vec<ActionResult>>,
    source: String,
}

fn emit_voice_phase(phase: &str) {
    if let Some(handle) = APP_HANDLE.get() {
        let _ = handle.emit("jarvis-voice-phase", phase);
    }
}

fn emit_chat_message(role: &str, content: &str, actions: Option<&[ActionResult]>) {
    if let Some(handle) = APP_HANDLE.get() {
        let payload = ChatStreamMessage {
            role: role.to_owned(),
            content: content.to_owned(),
            actions: actions.map(ToOwned::to_owned),
            source: "voice".to_owned(),
        };
        let _ = handle.emit("jarvis-chat-message", payload);
    }
}

pub fn set_app_handle(handle: AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

// ── Tauri commands ──────────────────────────────────────────────────

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
        SESSION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
        audio::clear_speaking_on_startup();
        stop_tracked_audio();
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

// ── Activation triggers ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Clap,
    Wake,
}

/// AEC leakage filter — cached ack phrases + short "…sir" utterances.
fn is_ack_leakage(text: &str) -> bool {
    let lower = text.to_lowercase();
    let lower = lower.trim_end_matches(['.', '?', '!', ',']);
    let lower = lower.trim();

    if matches!(
        lower,
        "yes" | "yes sir" | "yes, sir"
            | "ja" | "ja sir" | "ja, sir"
            | "right here, sir" | "right here sir"
            | "at your service, sir" | "at your service sir"
            | "listening, sir" | "listening sir"
            | "go ahead, sir" | "go ahead sir"
            | "i'm here, sir" | "i'm here sir" | "im here sir"
            | "standing by, sir" | "standing by sir"
    ) {
        return true;
    }
    // ≤4 words ending in "sir" = almost certainly ack leakage
    let words = lower.split_whitespace().count();
    words <= 4 && lower.ends_with("sir")
}

// ── Unified session flow ────────────────────────────────────────────

fn handle_activation(trigger: Trigger, guard: SpeakingGuard) {
    show_and_navigate_assistant();

    if trigger == Trigger::Clap {
        std::thread::spawn(|| {
            let _ = super::lighting::lighting_master_power(true);
        });
    }

    // Get user command — the two triggers differ only here
    let command = match trigger {
        Trigger::Clap => {
            emit_voice_phase("speaking");
            let entrance_path = pick_entrance_path();
            if let Err(e) = play_entrance_audio(&entrance_path) {
                log::warn!("[jarvis-clap] Entrance audio failed: {e}");
            }
            let _ = wait_for_entrance_playback(1_100, false);
            std::thread::sleep(std::time::Duration::from_millis(140));
            emit_voice_phase("listening");
            jarvis_listen_sync()
        }
        Trigger::Wake => {
            // Start STT immediately, but ignore exactly the ack duration.
            // This keeps the first user words while preventing ack self-capture.
            let ack = pick_cached_ack_path().ok();
            let ack_ignore_ms = ack
                .as_ref()
                .and_then(|p| wav_duration_ms(p))
                .map(|ms| ms.saturating_add(40))
                .unwrap_or(900);

            let stt = std::thread::Builder::new()
                .name("jarvis-wake-listen".into())
                .spawn(move || jarvis_listen_sync_with_ignore_ms(ack_ignore_ms))
                .expect("spawn wake-listen");

            emit_voice_phase("speaking");
            if let Some(path) = ack.as_ref() {
                if let Err(e) = play_cached_ack_file(path, true) {
                    log::warn!("[jarvis-wake] Cached ack failed: {e}");
                    let _ = tts_speak("Yes, Sir?", true);
                }
            } else if let Err(e) = play_cached_ack(true) {
                log::warn!("[jarvis-wake] Cached ack failed: {e}");
                let _ = tts_speak("Yes, Sir?", true);
            }

            emit_voice_phase("listening");
            match stt.join() {
                Ok(result) => result,
                Err(_) => Err("STT thread panicked".into()),
            }
        }
    };

    // Process the result
    let tag = if trigger == Trigger::Clap { "clap" } else { "wake" };
    match command {
        Ok(cmd) => {
            let cmd = cmd.trim().to_string();
            if cmd.is_empty() {
                emit_voice_phase("idle");
                log::info!("[jarvis-{tag}] No command captured");
                // guard drops → session ends, listener unmutes
                return;
            }
            if is_ack_leakage(&cmd) {
                emit_voice_phase("idle");
                log::info!("[jarvis-{tag}] Discarded AEC-leaked ack: {cmd}");
                return;
            }
            emit_voice_phase("processing");
            log::info!("[jarvis-{tag}] Command: {cmd}");
            run_voice_command(cmd, tag, guard);
        }
        Err(e) => {
            emit_voice_phase("idle");
            log::info!("[jarvis-{tag}] STT failed: {e}");
            // guard drops → session ends
        }
    }
}

/// Execute a voice command — LLM + TTS response.
/// Takes ownership of the SpeakingGuard so the session stays muted
/// through the entire command + response TTS.  Guard drops at the end.
fn run_voice_command(command: String, tag: &str, _guard: SpeakingGuard) {
    let start = std::time::Instant::now();
    let display_command = intent::canonicalize_voice_command(&command);
    emit_chat_message(
        "user",
        if display_command.is_empty() {
            &command
        } else {
            &display_command
        },
        None,
    );

    // Kill any lingering entrance audio
    kill_active_tts();

    let (resp, worker) = execute_voice_command(command, tag);
    if !resp.text.trim().is_empty() {
        emit_chat_message("assistant", &resp.text, Some(&resp.actions));
    }

    // Speak the response (guard keeps listener muted)
    if !resp.text.is_empty() {
        emit_voice_phase("speaking");
        if let Err(e) = tts_speak(&resp.text, true) {
            log::warn!("[jarvis-{tag}] TTS failed: {e}");
        }
    }

    // Wait for any async action workers
    if let Some(worker) = worker {
        match worker.join() {
            Ok(results) => {
                for r in results.iter().filter(|r| !r.success) {
                    log::warn!("[jarvis-{tag}] Action failed: {}", r.message);
                }
            }
            Err(_) => log::warn!("[jarvis-{tag}] Action worker panicked"),
        }
    }

    log::info!(
        "[jarvis-{tag}] Command complete ({}ms)",
        start.elapsed().as_millis()
    );
    emit_voice_phase("idle");
    // _guard drops here → file removed, SESSION_ACTIVE = false
}

// ── UI helpers ──────────────────────────────────────────────────────

fn show_and_navigate_assistant() {
    if let Some(handle) = APP_HANDLE.get() {
        if let Some(w) = handle.get_webview_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        let _ = handle.emit("navigate-assistant", ());

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

        // wmctrl fallback
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
}

// ── Listener event dispatch ─────────────────────────────────────────

fn handle_listener_event(event: ListenerEvent) {
    match event {
        ListenerEvent::Clap => {
            if SESSION_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            log::info!("[jarvis-listener] Clap detected");
            if let Some(guard) = SpeakingGuard::try_acquire() {
                std::thread::spawn(move || handle_activation(Trigger::Clap, guard));
            }
        }
        ListenerEvent::Wake => {
            if SESSION_ACTIVE.load(std::sync::atomic::Ordering::SeqCst) {
                return;
            }
            log::info!("[jarvis-listener] Wake word detected");
            if let Some(guard) = SpeakingGuard::try_acquire() {
                std::thread::spawn(move || handle_activation(Trigger::Wake, guard));
            }
        }
        ListenerEvent::Telemetry(telem) => {
            if let Some(handle) = APP_HANDLE.get() {
                let _ = handle.emit("jarvis-audio-telemetry", telem);
            }
        }
    }
}
