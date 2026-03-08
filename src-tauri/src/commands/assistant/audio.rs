//! Jarvis audio layer — TTS, STT, speaking gate, cached ack, entrance.
//!
//! Simplified design:
//! - **SpeakingGuard**: RAII guard — file created on acquire, removed on Drop.
//!   Owns both the marker file and the SESSION_ACTIVE flag.
//! - **PID tracking**: two AtomicU32s, two helpers.
//! - **TTS**: streaming only (Orpheus → WAV header → mpv), no batch fallback.
//! - **STT**: persistent `transcribe_once.py` worker.

use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use reqwest::blocking::Client;

// ── Constants ───────────────────────────────────────────────────────
const JARVIS_DATA_DIR: &str = "/home/max/.local/share/jarvis";
const ASSISTANT_SPEAKING_PATH: &str = "/home/max/.local/share/jarvis/assistant_speaking.txt";
const LISTEN_ONCE_SCRIPT: &str = "/home/max/.local/share/jarvis/transcribe_once.py";
const WHISPER_SERVER_URL: &str = "http://127.0.0.1:8178";
const ORPHEUS_TTS_HEALTH_URL: &str = "http://127.0.0.1:5005/";
const ORPHEUS_TTS_STREAM_URL: &str = "http://127.0.0.1:5005/v1/audio/speech/stream";
const ORPHEUS_TTS_SPEECH_URL: &str = "http://127.0.0.1:5005/v1/audio/speech";
const ACK_CACHE_DIR: &str = "/home/max/.local/share/jarvis/ack_cache";
const DEFAULT_STT_STARTUP_IGNORE_MS: u32 = 240;

// ── Shared state ────────────────────────────────────────────────────
/// True while a voice session (clap/wake → command → TTS) is running.
pub(crate) static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

static PLAYBACK_MUTE_COUNT: AtomicU32 = AtomicU32::new(0);
static TTS_PID: AtomicU32 = AtomicU32::new(0);
static ENTRANCE_PID: AtomicU32 = AtomicU32::new(0);
static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();
static TEMP_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

fn unique_nonce() -> u64 {
    TEMP_NONCE.fetch_add(1, Ordering::Relaxed)
}

// ═══════════════════════════════════════════════════════════════════
//  Speaking Guard — RAII, impossible to leak
// ═══════════════════════════════════════════════════════════════════

/// RAII guard for a voice session.  While alive `SESSION_ACTIVE` is true.
/// Actual audio playback mute is tracked separately.
pub(crate) struct SpeakingGuard {
    _private: (),
}

impl SpeakingGuard {
    /// Try to start a voice session.  Returns `None` if one is already active.
    pub(crate) fn try_acquire() -> Option<Self> {
        if SESSION_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            Some(Self { _private: () })
        } else {
            None
        }
    }
}

impl Drop for SpeakingGuard {
    fn drop(&mut self) {
        SESSION_ACTIVE.store(false, Ordering::SeqCst);
    }
}

/// Guard for actual audio playback only.
/// Keeps the listener muted while ack / entrance / TTS audio is playing.
struct PlaybackMuteGuard {
    _private: (),
}

impl PlaybackMuteGuard {
    fn acquire() -> Self {
        let prev = PLAYBACK_MUTE_COUNT.fetch_add(1, Ordering::SeqCst);
        if prev == 0 {
            let _ = std::fs::write(ASSISTANT_SPEAKING_PATH, "1");
        }
        Self { _private: () }
    }
}

impl Drop for PlaybackMuteGuard {
    fn drop(&mut self) {
        let prev = PLAYBACK_MUTE_COUNT.fetch_sub(1, Ordering::SeqCst);
        if prev <= 1 {
            PLAYBACK_MUTE_COUNT.store(0, Ordering::SeqCst);
            let _ = std::fs::remove_file(ASSISTANT_SPEAKING_PATH);
        }
    }
}

pub(crate) fn clear_speaking_on_startup() {
    SESSION_ACTIVE.store(false, Ordering::SeqCst);
    PLAYBACK_MUTE_COUNT.store(0, Ordering::SeqCst);
    let _ = std::fs::remove_file(ASSISTANT_SPEAKING_PATH);
}

// ═══════════════════════════════════════════════════════════════════
//  PID tracking
// ═══════════════════════════════════════════════════════════════════

fn kill_pid(slot: &AtomicU32) {
    let pid = slot.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let _ = Command::new("kill")
            .arg(pid.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn pid_alive(slot: &AtomicU32) -> bool {
    let pid = slot.load(Ordering::SeqCst);
    pid != 0 && std::path::Path::new(&format!("/proc/{pid}")).exists()
}

pub(crate) fn kill_active_tts() {
    kill_pid(&TTS_PID);
}

fn kill_active_entrance() {
    kill_pid(&ENTRANCE_PID);
}

pub(crate) fn stop_tracked_audio() {
    kill_active_entrance();
    kill_active_tts();
}

pub(crate) fn stop_music_playback() {
    stop_tracked_audio();
    let _ = Command::new("playerctl")
        .args(["--all-players", "stop"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// ═══════════════════════════════════════════════════════════════════
//  Orpheus TTS
// ═══════════════════════════════════════════════════════════════════

pub fn ensure_orpheus_tts_running() {
    if orpheus_tts_alive() {
        log::info!("[jarvis-tts] Orpheus TTS already running");
        return;
    }
    log::warn!("[jarvis-tts] Orpheus TTS not running — starting user services");
    for svc in ["orpheus-tts.service", "orpheus-fastapi.service"] {
        let _ = Command::new("systemctl")
            .args(["--user", "start", svc])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    for _ in 0..60 {
        if orpheus_tts_alive() {
            log::info!("[jarvis-tts] Orpheus TTS is online");
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    log::warn!("[jarvis-tts] Orpheus TTS failed to come online in time");
}

fn orpheus_tts_alive() -> bool {
    http_client()
        .get(ORPHEUS_TTS_HEALTH_URL)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .is_ok_and(|r| r.status().is_success())
}

// ── Streaming TTS ───────────────────────────────────────────────────

/// Speak text via Orpheus streaming → mpv.
/// Caller may hold a SpeakingGuard; playback mute is handled here.
pub(crate) fn tts_speak(text: &str, wait: bool) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("No text to speak".into());
    }

    let playback_mute = PlaybackMuteGuard::acquire();

    let payload = serde_json::json!({ "input": text, "voice": "leo" });
    let stream_client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("stream client: {e}"))?;

    let mut response = stream_client
        .post(ORPHEUS_TTS_STREAM_URL)
        .json(&payload)
        .send()
        .map_err(|e| format!("orpheus stream: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Orpheus stream error: {}", response.status()));
    }

    kill_active_tts();
    let mut player = Command::new("mpv")
        .args(["--no-video", "--really-quiet", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("mpv spawn: {e}"))?;

    let pid = player.id();
    TTS_PID.store(pid, Ordering::SeqCst);
    let mut stdin = player.stdin.take().ok_or("mpv: no stdin")?;

    // WAV header so mpv knows the format (s16le, 24kHz, mono)
    if stdin.write_all(&wav_header_24k_mono()).is_err() {
        let _ = player.wait();
        TTS_PID.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
        return Err("mpv closed before header".into());
    }

    let mut buf = [0u8; 8192];
    loop {
        match response.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if stdin.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
            Err(e) => {
                log::warn!("[jarvis-tts] Stream read error: {e}");
                break;
            }
        }
    }
    drop(stdin);

    if wait {
        let _ = player.wait();
        drop(playback_mute);
        TTS_PID.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
    } else {
        std::thread::spawn(move || {
            let _playback_mute = playback_mute;
            let _ = player.wait();
            TTS_PID.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
        });
    }
    Ok("ok".into())
}

/// Batch TTS — generate WAV bytes (gateway API only, not voice flow).
pub fn tts_generate_sync(text: &str) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("No text to speak".into());
    }
    let payload = serde_json::json!({ "input": text, "voice": "leo" });
    let response = http_client()
        .post(ORPHEUS_TTS_SPEECH_URL)
        .json(&payload)
        .send()
        .map_err(|e| format!("orpheus request: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let err = response.text().unwrap_or_default();
        return Err(format!("Orpheus TTS error ({status}): {err}"));
    }
    let data = response.bytes().map_err(|e| format!("read wav: {e}"))?.to_vec();
    if data.len() < 100 || &data[..4] != b"RIFF" {
        return Err(format!(
            "Orpheus returned invalid audio: {}",
            String::from_utf8_lossy(&data)
        ));
    }
    Ok(data)
}

/// Frontend-triggered TTS — creates its own temporary mute guard.
pub fn jarvis_speak_sync(text: String) -> Result<String, String> {
    tts_speak(&text, false)
}

fn wav_header_24k_mono() -> [u8; 44] {
    let max_data: u32 = 0x7FFF_FFFF;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + max_data).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&24000u32.to_le_bytes());
    h[28..32].copy_from_slice(&48000u32.to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&max_data.to_le_bytes());
    h
}

// ═══════════════════════════════════════════════════════════════════
//  Cached ack + entrance playback
// ═══════════════════════════════════════════════════════════════════

/// Play a random pre-cached ack WAV.  Caller owns the SpeakingGuard.
pub(crate) fn play_cached_ack(wait: bool) -> Result<(), String> {
    let path = pick_cached_ack_path()?;
    play_wav_file(&path, wait, &TTS_PID)
}

pub(crate) fn pick_cached_ack_path() -> Result<std::path::PathBuf, String> {
    let files = wav_files_in(ACK_CACHE_DIR)?;
    if files.is_empty() {
        return Err("No cached ack WAVs found".into());
    }
    Ok(files[nano_index(files.len())].clone())
}

pub(crate) fn play_cached_ack_file(path: &std::path::Path, wait: bool) -> Result<(), String> {
    play_wav_file(path, wait, &TTS_PID)
}

pub(crate) fn wav_duration_ms(path: &std::path::Path) -> Option<u32> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 44 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }

    let mut i = 12usize;
    let mut byte_rate: Option<u32> = None;
    let mut data_len: Option<u32> = None;

    while i + 8 <= data.len() {
        let chunk_id = &data[i..i + 4];
        let chunk_len = u32::from_le_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
        let chunk_data_start = i + 8;
        let chunk_data_end = chunk_data_start.saturating_add(chunk_len);
        if chunk_data_end > data.len() {
            break;
        }

        if chunk_id == b"fmt " {
            if chunk_len >= 8 {
                byte_rate = Some(u32::from_le_bytes(
                    data[chunk_data_start + 4..chunk_data_start + 8]
                        .try_into()
                        .ok()?,
                ));
            }
        } else if chunk_id == b"data" {
            data_len = Some(chunk_len as u32);
        }

        if byte_rate.is_some() && data_len.is_some() {
            break;
        }

        i = chunk_data_end + (chunk_len % 2);
    }

    let byte_rate = byte_rate?;
    let data_len = data_len?;
    if byte_rate == 0 {
        return None;
    }

    Some(((data_len as u64) * 1000 / (byte_rate as u64)) as u32)
}

pub(crate) fn pick_entrance_path() -> String {
    let seq_dir = format!("{JARVIS_DATA_DIR}/entrance_sequences");
    let files = wav_files_in(&seq_dir).unwrap_or_default();
    if files.is_empty() {
        format!("{JARVIS_DATA_DIR}/entrance.mp3")
    } else {
        files[nano_index(files.len())].to_string_lossy().to_string()
    }
}

pub(crate) fn play_entrance_audio(path: &str) -> Result<(), String> {
    kill_active_entrance();
    let playback_mute = PlaybackMuteGuard::acquire();
    let mut child = Command::new("mpv")
        .args(["--no-video", "--really-quiet", "--volume=80", path])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("entrance mpv: {e}"))?;

    let pid = child.id();
    ENTRANCE_PID.store(pid, Ordering::SeqCst);
    std::thread::spawn(move || {
        let _playback_mute = playback_mute;
        let _ = child.wait();
        ENTRANCE_PID.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
    });
    Ok(())
}

pub(crate) fn wait_for_entrance_playback(max_ms: u64, interrupt_on_timeout: bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_ms);
    while std::time::Instant::now() < deadline {
        if !pid_alive(&ENTRANCE_PID) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if interrupt_on_timeout && pid_alive(&ENTRANCE_PID) {
        kill_active_entrance();
        return true;
    }
    pid_alive(&ENTRANCE_PID)
}

// ── Helpers ─────────────────────────────────────────────────────────

fn wav_files_in(dir: &str) -> Result<Vec<std::path::PathBuf>, String> {
    Ok(std::fs::read_dir(dir)
        .map_err(|e| format!("read dir {dir}: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wav"))
        .collect())
}

fn nano_index(len: usize) -> usize {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize % len)
        .unwrap_or(0)
}

fn play_wav_file(
    path: &std::path::Path,
    wait: bool,
    pid_slot: &'static AtomicU32,
) -> Result<(), String> {
    kill_pid(pid_slot);
    let playback_mute = PlaybackMuteGuard::acquire();
    let path_str = path.to_string_lossy().to_string();
    let mut child = Command::new("mpv")
        .args(["--no-video", "--really-quiet", &path_str])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("mpv spawn: {e}"))?;

    let pid = child.id();
    pid_slot.store(pid, Ordering::SeqCst);

    if wait {
        let _ = child.wait();
        drop(playback_mute);
        pid_slot.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
    } else {
        std::thread::spawn(move || {
            let _playback_mute = playback_mute;
            let _ = child.wait();
            pid_slot.compare_exchange(pid, 0, Ordering::SeqCst, Ordering::SeqCst).ok();
        });
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
//  STT — persistent transcribe_once.py worker
// ═══════════════════════════════════════════════════════════════════

struct SttWorker {
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    #[allow(dead_code)]
    child: std::process::Child,
}

static STT_WORKER: std::sync::Mutex<Option<SttWorker>> = std::sync::Mutex::new(None);

pub fn ensure_stt_worker() -> Result<(), String> {
    let mut guard = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;

    if let Some(ref mut w) = *guard {
        match w.child.try_wait() {
            Ok(Some(_)) => {
                log::warn!("[stt-worker] Worker exited, respawning");
                *guard = None;
            }
            Ok(None) => return Ok(()),
            Err(_) => {
                *guard = None;
            }
        }
    }

    log::info!("[stt-worker] Spawning persistent STT worker");
    let mut child = Command::new("/home/max/.local/share/jarvis-stt-gpu/bin/python3")
        .arg(LISTEN_ONCE_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("STT spawn: {e}"))?;

    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("stt-worker-stderr".into())
            .spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    let t = line.trim();
                    if !t.is_empty()
                        && !t.contains("hipBLASLt on an unsupported architecture")
                        && t != "return F.linear("
                    {
                        log::info!("[stt-worker][py] {t}");
                    }
                }
            })
            .ok();
    }

    let stdin = child.stdin.take().ok_or("STT: no stdin")?;
    let mut stdout = std::io::BufReader::new(child.stdout.take().ok_or("STT: no stdout")?);

    let mut line = String::new();
    let start = std::time::Instant::now();
    loop {
        line.clear();
        match stdout.read_line(&mut line) {
            Ok(0) => return Err("STT worker exited during startup".into()),
            Ok(_) if line.trim() == "READY" => {
                log::info!("[stt-worker] Model loaded and ready");
                break;
            }
            Ok(_) => {}
            Err(e) => return Err(format!("STT startup read: {e}")),
        }
        if start.elapsed().as_secs() > 60 {
            return Err("STT worker startup timeout".into());
        }
    }

    *guard = Some(SttWorker {
        stdin,
        stdout,
        child,
    });
    Ok(())
}

pub fn jarvis_listen_sync() -> Result<String, String> {
    jarvis_listen_sync_with_ignore_ms(DEFAULT_STT_STARTUP_IGNORE_MS)
}

pub(crate) fn jarvis_listen_sync_with_ignore_ms(ignore_ms: u32) -> Result<String, String> {
    ensure_stt_worker()?;
    let mut guard = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;
    let worker = guard.as_mut().ok_or("STT worker not available")?;

    let ignore_ms = ignore_ms.max(DEFAULT_STT_STARTUP_IGNORE_MS);
    let go = format!("GO {ignore_ms}\n");

    worker
        .stdin
        .write_all(go.as_bytes())
        .map_err(|e| format!("STT write: {e}"))?;
    worker
        .stdin
        .flush()
        .map_err(|e| format!("STT flush: {e}"))?;

    let mut line = String::new();
    loop {
        line.clear();
        match worker.stdout.read_line(&mut line) {
            Ok(0) => {
                drop(guard);
                *STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))? = None;
                return Err("STT worker exited unexpectedly".into());
            }
            Ok(_) => {
                let trimmed = line.trim();
                if let Some(text) = trimmed.strip_prefix("TEXT:") {
                    let text = text.trim();
                    if !text.is_empty() {
                        return Ok(text.to_string());
                    }
                }
                if let Some(err) = trimmed.strip_prefix("ERROR:") {
                    return Err(err.trim().to_string());
                }
            }
            Err(e) => {
                drop(guard);
                *STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))? = None;
                return Err(format!("STT read: {e}"));
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
//  Gateway transcription (whisper.cpp direct HTTP)
// ═══════════════════════════════════════════════════════════════════

pub fn transcribe_audio_sync(audio_bytes: &[u8]) -> Result<String, String> {
    let ext = match audio_bytes.get(..8) {
        Some(b) if &b[4..8] == b"ftyp" => "m4a",
        Some(b) if &b[0..4] == b"RIFF" => "wav",
        Some(b) if &b[0..4] == [0x1A, 0x45, 0xDF, 0xA3] => "webm",
        Some(b) if &b[0..4] == b"OggS" => "ogg",
        _ => "wav",
    };
    let nonce = unique_nonce();
    let tmp = std::env::temp_dir().join(format!("jarvis_stt_{nonce}.{ext}"));
    std::fs::write(&tmp, audio_bytes).map_err(|e| format!("write stt: {e}"))?;
    let result = transcribe_via_whisper_server(tmp.to_str().unwrap_or("/tmp/jarvis_stt.wav"));
    let _ = std::fs::remove_file(&tmp);
    result
}

fn transcribe_via_whisper_server(wav_path: &str) -> Result<String, String> {
    let url = format!("{WHISPER_SERVER_URL}/inference");
    let file_bytes = std::fs::read(wav_path).map_err(|e| format!("read audio: {e}"))?;
    let file_name = std::path::Path::new(wav_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let file_part = reqwest::blocking::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("audio/wav")
        .map_err(|e| format!("multipart: {e}"))?;

    let form = reqwest::blocking::multipart::Form::new()
        .part("file", file_part)
        .text("response_format", "json")
        .text("language", "de");

    let resp = http_client()
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .map_err(|e| format!("whisper request: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("whisper error ({status}): {body}"));
    }

    let body: serde_json::Value = resp.json().map_err(|e| format!("whisper JSON: {e}"))?;
    let text = body["text"].as_str().unwrap_or("").trim().to_string();
    if text.is_empty() {
        return Err("Keine Sprache erkannt".into());
    }
    Ok(text)
}
