//! Gateway routes for Jarvis assistant — exposes chat + TTS + STT + voice via HTTP
//! so the mobile app can use Jarvis over the network.

use axum::{body::Bytes, routing::post, Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};

use crate::commands::assistant;

// ── Request / Response types ───────────────────────────────

#[derive(Deserialize)]
struct ChatReq {
    history: Vec<assistant::ChatMessage>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatResp {
    text: String,
    actions: Vec<assistant::ActionResult>,
}

#[derive(Deserialize)]
struct TtsReq {
    text: String,
}

#[derive(Serialize)]
struct TtsResp {
    /// base64-encoded WAV audio
    audio: String,
}

#[derive(Serialize)]
struct SttResp {
    text: String,
}

/// Combined voice assistant response: STT → LLM → TTS in one round-trip.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceResp {
    /// What the user said (STT result)
    transcription: String,
    /// Jarvis's spoken reply text
    text: String,
    /// Tool actions executed
    actions: Vec<assistant::ActionResult>,
    /// base64-encoded WAV of Jarvis's reply (for playback on phone)
    audio: String,
}

// ── Handlers ───────────────────────────────────────────────

async fn chat(Json(req): Json<ChatReq>) -> Json<ChatResp> {
    let resp: assistant::AssistantResponse =
        tokio::task::spawn_blocking(move || assistant::assistant_chat_sync(req.history))
            .await
            .unwrap_or_else(|e| assistant::AssistantResponse {
                text: format!("⚠️ Task error: {e}"),
                actions: vec![],
            });

    Json(ChatResp {
        text: resp.text,
        actions: resp.actions,
    })
}

/// Generate TTS and return base64-encoded WAV.
async fn tts(Json(req): Json<TtsReq>) -> Result<Json<TtsResp>, String> {
    let text = req.text;
    let wav_data = tokio::task::spawn_blocking(move || assistant::tts_generate_sync(&text))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e)?;

    Ok(Json(TtsResp {
        audio: B64.encode(&wav_data),
    }))
}

/// Transcribe uploaded WAV/audio to text via Whisper (GPU).
/// Expects raw audio bytes in the request body (Content-Type: audio/wav or application/octet-stream).
async fn stt(body: Bytes) -> Result<Json<SttResp>, String> {
    let wav_bytes = body.to_vec();
    let text = tokio::task::spawn_blocking(move || assistant::transcribe_audio_sync(&wav_bytes))
        .await
        .map_err(|e| format!("Task error: {e}"))?
        .map_err(|e| e)?;

    Ok(Json(SttResp { text }))
}

/// Combined voice endpoint: upload audio → STT → LLM → TTS → response with audio.
/// Expects raw WAV bytes in the request body.
async fn voice(body: Bytes) -> Result<Json<VoiceResp>, String> {
    let wav_bytes = body.to_vec();

    // 1. STT: transcribe the audio
    let transcription = tokio::task::spawn_blocking({
        let bytes = wav_bytes.clone();
        move || assistant::transcribe_audio_sync(&bytes)
    })
    .await
    .map_err(|e| format!("STT task error: {e}"))?
    .map_err(|e| e)?;

    // 2. LLM: process the command
    let transcript_clone = transcription.clone();
    let resp = tokio::task::spawn_blocking(move || {
        let msg = assistant::ChatMessage {
            role: "user".into(),
            content: transcript_clone,
        };
        assistant::assistant_chat_sync(vec![msg])
    })
    .await
    .unwrap_or_else(|e| assistant::AssistantResponse {
        text: format!("⚠️ Task error: {e}"),
        actions: vec![],
    });

    // 3. TTS: generate audio reply
    let reply_text = resp.text.clone();
    let audio_b64 = if !reply_text.is_empty() {
        tokio::task::spawn_blocking(move || {
            assistant::tts_generate_sync(&reply_text)
                .map(|wav| B64.encode(&wav))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    } else {
        String::new()
    };

    Ok(Json(VoiceResp {
        transcription,
        text: resp.text,
        actions: resp.actions,
        audio: audio_b64,
    }))
}

async fn status() -> Json<serde_json::Value> {
    let (ok, msg) = tokio::task::spawn_blocking(assistant::assistant_status_sync)
        .await
        .unwrap_or((false, "Task error".into()));
    Json(serde_json::json!({ "online": ok, "message": msg }))
}

// ── Router ─────────────────────────────────────────────────

pub fn routes() -> Router {
    Router::new()
        .route("/assistant/chat", post(chat))
        .route("/assistant/tts", post(tts))
        .route("/assistant/stt", post(stt))
        .route("/assistant/voice", post(voice))
        .route("/assistant/status", axum::routing::get(status))
}
