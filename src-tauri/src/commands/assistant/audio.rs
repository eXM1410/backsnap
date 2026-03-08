use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use reqwest::blocking::Client;

static ACTIVE_TTS_PID: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);
static ACTIVE_ENTRANCE_PID: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);
static TEMP_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

const JARVIS_DATA_DIR: &str = "/home/max/.local/share/jarvis";
const ASSISTANT_SPEAKING_PATH: &str = "/home/max/.local/share/jarvis/assistant_speaking.txt";
const LISTEN_ONCE_SCRIPT: &str = "/home/max/.local/share/jarvis/transcribe_once.py";
const WHISPER_SERVER_URL: &str = "http://127.0.0.1:8178";
const ORPHEUS_TTS_HEALTH_URL: &str = "http://127.0.0.1:5005/";
const ORPHEUS_TTS_SPEECH_URL: &str = "http://127.0.0.1:5005/v1/audio/speech";
const ORPHEUS_TTS_STREAM_URL: &str = "http://127.0.0.1:5005/v1/audio/speech/stream";

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn http_client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        match Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                log::warn!("[jarvis-http] failed to build configured client, falling back: {err}");
                Client::new()
            }
        }
    })
}

fn unique_nonce() -> u64 {
    TEMP_NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

// ── Speaking flag (refcounted) ──────────────────────────────────────
// inc_speaking() / dec_speaking() maintain a counter.
// File exists ⇔ counter > 0.  Python just checks os.path.exists().
static SPEAKING_REFCOUNT: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

fn inc_speaking() {
    let prev = SPEAKING_REFCOUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if prev == 0 {
        let _ = std::fs::write(ASSISTANT_SPEAKING_PATH, "1");
    }
}

fn dec_speaking() {
    let prev = SPEAKING_REFCOUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    if prev <= 1 {
        SPEAKING_REFCOUNT.fetch_max(0, std::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(ASSISTANT_SPEAKING_PATH);
    }
}

pub(crate) fn inc_speaking_guard() {
    inc_speaking();
}

pub(crate) fn dec_speaking_guard() {
    dec_speaking();
}

pub(crate) fn clear_speaking_on_startup() {
    SPEAKING_REFCOUNT.store(0, std::sync::atomic::Ordering::SeqCst);
    let _ = std::fs::remove_file(ASSISTANT_SPEAKING_PATH);
}

fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

fn store_pid(slot: &'static std::sync::Mutex<Option<u32>>, pid: u32) {
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(pid);
    }
}

fn clear_pid_if(slot: &'static std::sync::Mutex<Option<u32>>, pid: u32) {
    if let Ok(mut guard) = slot.lock() {
        if guard.as_ref().copied() == Some(pid) {
            *guard = None;
        }
    }
}

fn tracked_pid(slot: &'static std::sync::Mutex<Option<u32>>) -> Option<u32> {
    slot.lock().ok().and_then(|guard| *guard)
}

fn tracked_pid_playing(slot: &'static std::sync::Mutex<Option<u32>>) -> bool {
    tracked_pid(slot).is_some_and(process_exists)
}

fn kill_tracked_pid(slot: &'static std::sync::Mutex<Option<u32>>) {
    if let Ok(mut guard) = slot.lock() {
        if let Some(pid) = guard.take() {
            let _ = Command::new("kill")
                .arg(pid.to_string())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

pub(crate) fn kill_active_tts() {
    kill_tracked_pid(&ACTIVE_TTS_PID);
}

pub(crate) fn kill_active_entrance() {
    kill_tracked_pid(&ACTIVE_ENTRANCE_PID);
}

fn entrance_playing() -> bool {
    tracked_pid_playing(&ACTIVE_ENTRANCE_PID)
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

pub fn ensure_orpheus_tts_running() {
    if orpheus_tts_alive() {
        log::info!("[jarvis-tts] Orpheus TTS already running");
        return;
    }

    log::warn!("[jarvis-tts] Orpheus TTS not running — starting user services");
    let _ = Command::new("systemctl")
        .args(["--user", "start", "orpheus-tts.service"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "start", "orpheus-fastapi.service"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

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
        .map(|response| response.status().is_success())
        .unwrap_or(false)
}

pub fn jarvis_speak_sync(text: String) -> Result<String, String> {
    tts_speak(&text, false)
}

pub fn tts_generate_sync(text: &str) -> Result<Vec<u8>, String> {
    if text.trim().is_empty() {
        return Err("No text to speak".into());
    }

    let payload = serde_json::json!({
        "input": text,
        "voice": "leo"
    });

    let response = http_client()
        .post(ORPHEUS_TTS_SPEECH_URL)
        .json(&payload)
        .send()
        .map_err(|e| format!("orpheus request: {e}"))?;
    if !response.status().is_success() {
        let status = response.status();
        let err = response
            .text()
            .unwrap_or_else(|_| String::from("failed to read error body"));
        return Err(format!("Orpheus TTS error ({status}): {err}"));
    }

    let data = response
        .bytes()
        .map_err(|e| format!("read wav body: {e}"))?
        .to_vec();

    if data.len() < 100 || &data[..4] != b"RIFF" {
        let body = String::from_utf8_lossy(&data);
        return Err(format!("Orpheus returned invalid audio: {body}"));
    }

    Ok(data)
}

fn tts_play_wav(wav_data: &[u8], wait: bool) -> Result<(), String> {
    let nonce = unique_nonce();
    let tmp = std::env::temp_dir().join(format!("jarvis_tts_{nonce}.wav"));
    std::fs::write(&tmp, wav_data).map_err(|e| format!("write wav: {e}"))?;

    kill_active_tts();

    let tmp_str = tmp.to_string_lossy().to_string();
    let mut child = Command::new("mpv")
        .args(["--no-video", "--really-quiet", &tmp_str])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("mpv spawn: {e}"))?;

    let pid = child.id();
    store_pid(&ACTIVE_TTS_PID, pid);

    if wait {
        let _ = child.wait();
        clear_pid_if(&ACTIVE_TTS_PID, pid);
        dec_speaking();
        let _ = std::fs::remove_file(&tmp);
    } else {
        std::thread::spawn(move || {
            let _ = child.wait();
            clear_pid_if(&ACTIVE_TTS_PID, pid);
            dec_speaking();
            let _ = std::fs::remove_file(tmp);
        });
    }

    Ok(())
}

pub(crate) fn tts_speak(text: &str, wait: bool) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("No text to speak".into());
    }
    // Mark speaking BEFORE generation so there's no gap where
    // Python might hear residual audio from a prior speech turn.
    inc_speaking();

    // Try streaming first (audio starts ~300ms after request vs ~3s batch)
    match tts_speak_streaming(text, wait) {
        Ok(result) => return Ok(result),
        Err(e) => {
            log::warn!("[jarvis-tts] Streaming TTS failed, falling back to batch: {e}");
        }
    }

    // Fallback: batch generate + play
    let wav_data = match tts_generate_sync(text) {
        Ok(d) => d,
        Err(e) => {
            dec_speaking();
            return Err(e);
        }
    };
    tts_play_wav(&wav_data, wait).map_err(|e| {
        dec_speaking();
        e
    })?;
    Ok("ok".into())
}

/// Stream raw PCM from Orpheus directly into mpv — first audio plays
/// after ~300ms instead of waiting for the full WAV generation (~3s).
/// Uses mpv instead of pw-cat because pw-cat silently loses audio on
/// repeated invocations (PipeWire/WirePlumber AEC routing bug).
fn tts_speak_streaming(text: &str, wait: bool) -> Result<String, String> {
    let payload = serde_json::json!({
        "input": text,
        "voice": "leo"
    });

    // Longer timeout for streaming (initial connection + full generation)
    let stream_client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("stream client: {e}"))?;

    let mut response = stream_client
        .post(ORPHEUS_TTS_STREAM_URL)
        .json(&payload)
        .send()
        .map_err(|e| format!("orpheus stream request: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Orpheus stream error: {status}"));
    }

    // Spawn mpv for raw PCM playback: s16le 24kHz mono via stdin
    let mut player = Command::new("mpv")
        .args([
            "--demuxer=rawaudio",
            "--demuxer-rawaudio-format=s16le",
            "--demuxer-rawaudio-rate=24000",
            "--demuxer-rawaudio-channels=1",
            "--no-terminal",
            "--no-video",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("mpv spawn: {e}"))?;

    let player_pid = player.id();
    store_pid(&ACTIVE_TTS_PID, player_pid);

    let mut player_stdin = player.stdin.take().ok_or("mpv: no stdin")?;

    // Stream HTTP response body → mpv stdin
    let mut buf = [0u8; 8192];
    loop {
        match response.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if player_stdin.write_all(&buf[..n]).is_err() {
                    break; // mpv was killed (e.g. kill_active_tts)
                }
            }
            Err(e) => {
                log::warn!("[jarvis-tts] Stream read error: {e}");
                break;
            }
        }
    }

    // Close stdin → mpv drains buffer and exits
    drop(player_stdin);

    if wait {
        let _ = player.wait();
        clear_pid_if(&ACTIVE_TTS_PID, player_pid);
        dec_speaking();
    } else {
        std::thread::spawn(move || {
            let _ = player.wait();
            clear_pid_if(&ACTIVE_TTS_PID, player_pid);
            dec_speaking();
        });
    }

    Ok("ok".into())
}

const ACK_CACHE_DIR: &str = "/home/max/.local/share/jarvis/ack_cache";

/// Play a random pre-cached acknowledgement WAV (e.g. "Yes, Sir?").
/// Much faster than TTS generation (~50ms vs ~800ms).
pub(crate) fn play_cached_ack(wait: bool) -> Result<(), String> {
    let files: Vec<_> = std::fs::read_dir(ACK_CACHE_DIR)
        .map_err(|e| format!("read ack_cache: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false))
        .collect();

    if files.is_empty() {
        return Err("No cached ack WAVs found".into());
    }

    let idx = {
        use std::time::SystemTime;
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos() as usize % files.len())
            .unwrap_or(0)
    };

    let path = &files[idx];
    let wav_data = std::fs::read(path).map_err(|e| format!("read ack wav: {e}"))?;

    inc_speaking();
    tts_play_wav(&wav_data, wait).map_err(|e| {
        dec_speaking();
        e
    })?;
    Ok(())
}

pub(crate) fn pick_entrance_path() -> String {
    let seq_dir = format!("{JARVIS_DATA_DIR}/entrance_sequences");
    let entrance_files: Vec<_> = std::fs::read_dir(&seq_dir)
        .ok()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();

    if entrance_files.is_empty() {
        format!("{JARVIS_DATA_DIR}/entrance.mp3")
    } else {
        use std::time::SystemTime;
        let idx = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis() as usize % entrance_files.len())
            .unwrap_or(0);
        entrance_files[idx].to_string_lossy().to_string()
    }
}

pub(crate) fn play_entrance_audio(path: &str) -> Result<(), String> {
    inc_speaking();
    kill_active_entrance();

    let mut child = Command::new("mpv")
        .args(["--no-video", "--really-quiet", "--volume=80", path])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            dec_speaking();
            format!("entrance mpv spawn: {e}")
        })?;

    let pid = child.id();
    store_pid(&ACTIVE_ENTRANCE_PID, pid);

    std::thread::spawn(move || {
        let _ = child.wait();
        clear_pid_if(&ACTIVE_ENTRANCE_PID, pid);
        dec_speaking();
    });

    Ok(())
}

pub(crate) fn wait_for_entrance_playback(max_wait_ms: u64, interrupt_on_timeout: bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(max_wait_ms);
    while std::time::Instant::now() < deadline {
        if !entrance_playing() {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if interrupt_on_timeout && entrance_playing() {
        kill_active_entrance();
        return true;
    }

    entrance_playing()
}

pub fn transcribe_audio_sync(audio_bytes: &[u8]) -> Result<String, String> {
    let ext = if audio_bytes.len() >= 8 && &audio_bytes[4..8] == b"ftyp" {
        "m4a"
    } else if audio_bytes.len() >= 4 && &audio_bytes[0..4] == b"RIFF" {
        "wav"
    } else if audio_bytes.len() >= 4 && &audio_bytes[0..4] == [0x1A, 0x45, 0xDF, 0xA3] {
        "webm"
    } else if audio_bytes.len() >= 4 && &audio_bytes[0..4] == b"OggS" {
        "ogg"
    } else {
        "wav"
    };
    let nonce = unique_nonce();
    let tmp = std::env::temp_dir().join(format!("jarvis_stt_{nonce}.{ext}"));
    std::fs::write(&tmp, audio_bytes).map_err(|e| format!("write stt audio: {e}"))?;

    let result = transcribe_via_whisper_server(tmp.to_str().unwrap_or("/tmp/jarvis_stt.wav"));
    let _ = std::fs::remove_file(&tmp);
    result
}

/// POST audio file to whisper.cpp server and return transcribed text.
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
        .map_err(|e| format!("multipart mime: {e}"))?;

    let form = reqwest::blocking::multipart::Form::new()
        .part("file", file_part)
        .text("response_format", "json")
        .text("language", "de");

    let resp = http_client()
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .map_err(|e| format!("whisper-server request: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(format!("whisper-server error ({status}): {body}"));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| format!("whisper-server JSON: {e}"))?;

    let text = body["text"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if text.is_empty() {
        return Err("Keine Sprache erkannt".into());
    }
    Ok(text)
}

/// Persistent STT worker — model loaded once, reused across requests.
struct SttWorker {
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    #[allow(dead_code)]
    child: std::process::Child,
}

static STT_WORKER: std::sync::Mutex<Option<SttWorker>> = std::sync::Mutex::new(None);

pub fn ensure_stt_worker() -> Result<(), String> {
    let mut guard = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;

    // Check if existing worker is still alive
    if let Some(ref mut w) = *guard {
        // Try a quick check — if child exited, we need a new one
        match w.child.try_wait() {
            Ok(Some(_)) => {
                log::warn!("[stt-worker] Worker process exited, respawning");
                *guard = None;
            }
            Ok(None) => return Ok(()), // still running
            Err(_) => {
                *guard = None;
            }
        }
    }

    log::info!("[stt-worker] Spawning persistent STT worker");
    let mut child = Command::new("python3")
        .arg(LISTEN_ONCE_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("STT spawn: {e}"))?;

    let stderr = child.stderr.take();
    if let Some(stderr) = stderr {
        std::thread::Builder::new()
            .name("stt-worker-stderr".into())
            .spawn(move || {
                let reader = std::io::BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(l) if !l.trim().is_empty() => {
                            let t = l.trim();
                            if !t.contains("hipBLASLt on an unsupported architecture")
                                && t != "return F.linear("
                            {
                                log::info!("[stt-worker][py] {t}");
                            }
                        }
                        _ => {}
                    }
                }
            })
            .ok();
    }

    let stdin = child.stdin.take().ok_or("STT: no stdin")?;
    let stdout_raw = child.stdout.take().ok_or("STT: no stdout")?;
    let mut stdout = std::io::BufReader::new(stdout_raw);

    // Wait for READY signal (model loaded)
    let mut ready_line = String::new();
    let start = std::time::Instant::now();
    loop {
        ready_line.clear();
        match stdout.read_line(&mut ready_line) {
            Ok(0) => return Err("STT worker exited during startup".into()),
            Ok(_) => {
                if ready_line.trim() == "READY" {
                    log::info!("[stt-worker] Model loaded and ready");
                    break;
                }
            }
            Err(e) => return Err(format!("STT startup read: {e}")),
        }
        if start.elapsed().as_secs() > 60 {
            return Err("STT worker startup timeout".into());
        }
    }

    *guard = Some(SttWorker { stdin, stdout, child });
    Ok(())
}

pub fn jarvis_listen_sync() -> Result<String, String> {
    ensure_stt_worker()?;
    let mut guard = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;
    let worker = guard.as_mut().ok_or("STT worker not available")?;

    // Send GO signal
    worker
        .stdin
        .write_all(b"GO\n")
        .map_err(|e| format!("STT write: {e}"))?;
    worker
        .stdin
        .flush()
        .map_err(|e| format!("STT flush: {e}"))?;

    // Read lines until we get TEXT: or ERROR:
    let mut line = String::new();
    loop {
        line.clear();
        match worker.stdout.read_line(&mut line) {
            Ok(0) => {
                // Worker died — drop it so next call respawns
                drop(guard);
                let mut g = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;
                *g = None;
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
                // LISTENING / TRANSCRIBING status lines — ignore
            }
            Err(e) => {
                drop(guard);
                let mut g = STT_WORKER.lock().map_err(|e| format!("STT lock: {e}"))?;
                *g = None;
                return Err(format!("STT read: {e}"));
            }
        }
    }
}
