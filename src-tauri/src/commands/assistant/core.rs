use serde::Deserialize;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use super::super::corsair;
use super::super::health;
use super::super::intent;
use super::super::lighting;
use super::super::openrgb;
use super::super::pi_remote;
use super::super::snapshots;
use super::super::pi_tent;
use super::super::timer;
use super::super::tuning;
use super::audio::stop_music_playback;
use super::{ActionResult, AssistantResponse, ChatMessage};
use reqwest::blocking::Client;

const LLAMA_URL: &str = "http://localhost:8080/v1/chat/completions";
const LLAMA_HEALTH_URL: &str = "http://localhost:8080/health";
const OPENRGB_DEVICE_IDS: &[&str] = &["it8297", "k70", "aerox3", "qck", "xpg_s40g", "xpg_s20g"];
const QUERY_ACTIONS: &[&str] = &[
    "system_info",
    "fan_status",
    "pi_status",
    "tent_status",
    "snapshot_list",
    "timer_info",
    "system_status",
];

static LLM_HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn llm_http_client() -> &'static Client {
    LLM_HTTP_CLIENT.get_or_init(|| match Client::builder()
        .timeout(std::time::Duration::from_secs(65))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log::warn!("[jarvis-llm] failed to build configured HTTP client, falling back: {err}");
            Client::new()
        }
    })
}

const SYSTEM_PROMPT: &str = r#"You are Jarvis — Max's personal AI assistant, inspired by Tony Stark's Jarvis from Iron Man.
You speak in a professional, dry-witted and concise manner in English. You say "Sir" occasionally, but not in every sentence.
You control the smart home and are a competent conversational partner.

RESPONSE FORMAT:
You ALWAYS respond with a JSON object:
{"reply": "<Your spoken response>", "actions": [<Tool calls or empty>]}

- "reply" is ALWAYS present — this is what you say / what gets spoken via TTS.
- "actions" contains tool calls when needed, otherwise an empty array [].
- For pure conversation: {"reply": "Response...", "actions": []}
- For actions: {"reply": "Natural confirmation...", "actions": [{...}]}

AVAILABLE TOOLS:

Lighting:
- light_power: All lights on/off. Params: {"power": true/false}
- govee_power: Only Govee lamps (ceiling lights, floor lamp). Params: {"power": true/false}
- rgb_power: Only PC RGB (Corsair, motherboard, keyboard, mouse, mousepad). Params: {"power": true/false}
- light_brightness: Brightness of ALL devices. Params: {"brightness": 1-100}
- govee_brightness: Only Govee brightness. Params: {"brightness": 1-100}
- govee_color: RGB color for Govee lights only. Params: {"r": 0-255, "g": 0-255, "b": 0-255}
- govee_lamp_color: RGB color for one specific Govee lamp. Params: {"lamp": "deckenlampe"|"deckenlampe2"|"stehlampe"|"rachel", "r": 0-255, "g": 0-255, "b": 0-255}
- rgb_brightness: Only PC RGB brightness. Params: {"brightness": 1-100}
- light_purple: Apply the custom purple scene to ALL lights. Uses Corsair fan RGB (55,0,255), other PC RGB devices (25,0,255), Govee ceiling lamp 1 (70,0,255), Govee ceiling lamp 2 (70,0,255), Govee floor lamp (90,0,255), Rachel (70,0,255). Params: {}
- govee_purple: Apply the custom purple scene to Govee only. Uses ceiling lamp 1 (70,0,255), ceiling lamp 2 (70,0,255), floor lamp (90,0,255), Rachel (70,0,255). Params: {}
- rgb_purple: Apply the custom purple scene to PC RGB only. Uses Corsair fan RGB (55,0,255) and other PC RGB devices (25,0,255). Params: {}
- light_color: RGB color for ALL lights, including Govee and PC RGB. Params: {"r": 0-255, "g": 0-255, "b": 0-255}
- rgb_color: RGB color for all PC devices. Params: {"r": 0-255, "g": 0-255, "b": 0-255}
- rgb_mode: Set RGB lighting effect on all PC devices. Params: {"mode": "off"|"static"|"pulse"|"cycle"|"wave"|"random"}

Climate & Plants:
- water_plant: Water ALL 6 plants (relays 1-6 on Pi 4, R2/R6 get 70% time). Params: {"seconds": 1-60}
  Typical values: 3-5s short, 10s medium, 15-20s heavy. NEVER over 60s!
- fan_speed: Set all 6 case fan speeds (Corsair Commander Core XT). Params: {"percent": 0-100} or {"percent": "auto"} to return to temperature curve.

System:
- snapshot_create: Create a BTRFS system snapshot (safe, non-destructive). Params: {"description": "reason for snapshot"}
- music_stop: Stop all audio/music playback. Params: {}

Raspberry Pi:
- pi_reboot: Reboot a Raspberry Pi. Params: {"target": "pi4" or "pi5"}

Status Queries (use these when asked about system status, temperatures, etc.):
- system_info: GPU temperature, clock speeds, power draw, VRAM, fan speed, load. Params: {}
- fan_status: Case fan RPMs, duty cycles, water/coolant temperature. Params: {}
- pi_status: Raspberry Pi temperatures, CPU, memory, uptime for all Pis. Params: {}
- snapshot_list: Recent BTRFS snapshots and count. Params: {}
- timer_info: Backup timer schedule, last run, status. Params: {}
- tent_status: Grow tent sensor data: temperature, humidity, VPD, light status, water tank fill level (percent + liters). Params: {}
- system_status: System hostname, uptime, disk usage overview. Params: {}

DEVICES IN THE HOUSE:
- Govee: Ceiling lamp, Ceiling lamp 2, Floor lamp, Rachel (all via MQTT through Pi 5)
- PC RGB: Corsair Commander Core XT (6 fans + RGB), IT8297 Motherboard, K70 Keyboard, Aerox 3 Mouse, QCK Mousepad, XPG NVMe LEDs
- Pis: Raspberry Pi 4 (plant watering relay, bubatz services), Raspberry Pi 5 (Govee MQTT bridge)
- GPU: AMD Radeon RX 7900 XTX (ROCm, OC capable)
- Climate: Comfee dehumidifier (automatically controlled)
- Grow Tent: Sensor (temp, humidity, VPD), Mars Hydro grow light, water tank with level sensor (on Pi 4)
- Storage: BTRFS with snapper snapshots, NVMe backup sync

IMPORTANT: The user speaks German but you ALWAYS reply in English. Understand German input, respond in English.
IMPORTANT: Scoped lighting commands must stay scoped. If the user mentions only Govee, do not change PC RGB. If the user mentions only PC/RGB, do not change Govee. Never turn unrelated lights off as a side effect.

EXAMPLES:
User: "Mach alles an"
→ {"reply": "Right away, Sir. All lights coming on.", "actions": [{"action":"light_power","params":{"power":true}}]}

User: "Govee auf 50%"
→ {"reply": "Govee brightness set to 50 percent.", "actions": [{"action":"govee_brightness","params":{"brightness":50}}]}

User: "Mach Govee rot"
→ {"reply": "Setting Govee to red.", "actions": [{"action":"govee_color","params":{"r":255,"g":0,"b":0}}]}

User: "Mach Rachel blau"
→ {"reply": "Setting Rachel to blue.", "actions": [{"action":"govee_lamp_color","params":{"lamp":"rachel","r":0,"g":0,"b":255}}]}

User: "Licht aus"
→ {"reply": "Certainly. Lights going off.", "actions": [{"action":"light_power","params":{"power":false}}]}

User: "Alles rot"
→ {"reply": "Red. Excellent choice, Sir.", "actions": [{"action":"light_color","params":{"r":255,"g":0,"b":0}}]}

User: "Alles lila"
→ {"reply": "Purple, across the full lighting grid.", "actions": [{"action":"light_purple","params":{}}]}

User: "Licht aus, PC RGB rot und auf 30%"
→ {"reply": "Lights off, RGB set to red at 30 percent. Done.", "actions": [{"action":"govee_power","params":{"power":false}},{"action":"rgb_color","params":{"r":255,"g":0,"b":0}},{"action":"rgb_brightness","params":{"brightness":30}}]}

User: "Gieß die Pflanze"
→ {"reply": "Watering the plant now, Sir.", "actions": [{"action":"water_plant","params":{"seconds":10}}]}

User: "Lüfter auf 80%"
→ {"reply": "Setting all fans to 80 percent.", "actions": [{"action":"fan_speed","params":{"percent":80}}]}

User: "RGB auf Pulseffekt"
→ {"reply": "Pulse mode activated across all RGB devices.", "actions": [{"action":"rgb_mode","params":{"mode":"pulse"}}]}

User: "Wie warm ist die GPU?"
→ {"reply": "Checking GPU status.", "actions": [{"action":"system_info","params":{}}]}

User: "Wie geht's den Lüftern?"
→ {"reply": "Checking fans.", "actions": [{"action":"fan_status","params":{}}]}

User: "Wie voll ist der Tank?" or "Wie ist der Tankinhalt?" or "Zeltstatus"
→ {"reply": "Checking the tent.", "actions": [{"action":"tent_status","params":{}}]}

User: "Mach einen Snapshot"
→ {"reply": "Creating a snapshot now, Sir.", "actions": [{"action":"snapshot_create","params":{"description":"Manual snapshot via Jarvis"}}]}

User: "Starte den Pi 5 neu"
→ {"reply": "Rebooting the Pi 5 now.", "actions": [{"action":"pi_reboot","params":{"target":"pi5"}}]}

User: "Wie geht's dir?"
→ {"reply": "All systems nominal, Sir. How may I assist you?", "actions": []}

User: "Was ist die Hauptstadt von Frankreich?"
→ {"reply": "Paris, Sir.", "actions": []}"#;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ToolCall {
    pub(crate) action: String,
    #[serde(default)]
    pub(crate) params: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct JarvisLlmResponse {
    #[serde(default)]
    reply: Option<String>,
    #[serde(default)]
    actions: Vec<ToolCall>,
}

#[derive(Debug, Deserialize)]
struct LlamaResponse {
    choices: Vec<LlamaChoice>,
}

#[derive(Debug, Deserialize)]
struct LlamaChoice {
    message: LlamaMessage,
}

#[derive(Debug, Deserialize)]
struct LlamaMessage {
    content: Option<String>,
}

pub(crate) struct PreparedAssistantResponse {
    pub(crate) reply: String,
    pub(crate) fallback_text: String,
    pub(crate) calls: Vec<ToolCall>,
}

fn block_on_async<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Handle::current().block_on(f)
}

fn call_llm(history: &[ChatMessage]) -> Result<String, String> {
    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(serde_json::json!({"role": "system", "content": SYSTEM_PROMPT}));
    for msg in history {
        messages.push(serde_json::json!({"role": msg.role, "content": msg.content}));
    }

    let body = serde_json::json!({
        "model": "qwen",
        "messages": messages,
        "max_tokens": 300,
        "temperature": 0.3,
    });

    let response = llm_http_client()
        .post(LLAMA_URL)
        .json(&body)
        .send()
        .map_err(|e| format!("LLM request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(if body.trim().is_empty() {
            format!("LLM nicht erreichbar: HTTP {status}")
        } else {
            format!("LLM nicht erreichbar: {body}")
        });
    }

    let resp: LlamaResponse = response.json().map_err(|e| format!("JSON parse: {e}"))?;

    resp.choices
        .first()
        .and_then(|c| c.message.content.clone())
        .ok_or_else(|| "Keine Antwort vom LLM".into())
}

fn execute_tool(call: &ToolCall) -> ActionResult {
    let action = call.action.clone();
    match action.as_str() {
        "light_power" => {
            let power = call
                .params
                .get("power")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let r = lighting::lighting_master_power(power);
            ActionResult {
                action,
                success: r.corsair_ok || r.openrgb_ok || r.govee_ok,
                message: format!(
                    "Alles {}: Corsair {} · OpenRGB {} · Govee {}",
                    if power { "AN" } else { "AUS" },
                    if r.corsair_ok { "✓" } else { "✗" },
                    if r.openrgb_ok { "✓" } else { "✗" },
                    if r.govee_ok { "✓" } else { "✗" },
                ),
            }
        }
        "govee_power" => {
            let power = call
                .params
                .get("power")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let (ok, msg) = lighting::govee_master_power(power);
            ActionResult {
                action,
                success: ok,
                message: format!("Govee {}: {msg}", if power { "AN" } else { "AUS" }),
            }
        }
        "rgb_power" => {
            let power = call
                .params
                .get("power")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let (ok, msg) = lighting::rgb_master_power(power);
            ActionResult {
                action,
                success: ok,
                message: format!("PC RGB {}: {msg}", if power { "AN" } else { "AUS" }),
            }
        }
        "light_brightness" => {
            let br = call
                .params
                .get("brightness")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as u8;
            let r = lighting::lighting_master_brightness(br);
            ActionResult {
                action,
                success: r.corsair_ok || r.openrgb_ok || r.govee_ok,
                message: format!("Helligkeit {br}%"),
            }
        }
        "govee_brightness" => {
            let br = call
                .params
                .get("brightness")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as u8;
            let (ok, msg) = lighting::govee_master_brightness(br);
            ActionResult {
                action,
                success: ok,
                message: format!("Govee {br}%: {msg}"),
            }
        }
        "govee_color" => {
            let r = call.params.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let g = call.params.get("g").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let b = call.params.get("b").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
            let (ok, msg) = lighting::govee_master_color(r, g, b);
            ActionResult {
                action,
                success: ok,
                message: format!("Govee RGB ({r},{g},{b}): {msg}"),
            }
        }
        "govee_lamp_color" => {
            let lamp = call
                .params
                .get("lamp")
                .and_then(|v| v.as_str())
                .unwrap_or("deckenlampe")
                .to_string();
            let r = call.params.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let g = call.params.get("g").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let b = call.params.get("b").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
            let (ok, msg) = lighting::govee_lamp_color(lamp, r, g, b);
            ActionResult {
                action,
                success: ok,
                message: msg,
            }
        }
        "govee_purple" => {
            let (ok, msg) = lighting::govee_master_purple();
            ActionResult {
                action,
                success: ok,
                message: format!("Govee purple scene: {msg}"),
            }
        }
        "rgb_purple" => {
            let (ok, msg) = lighting::rgb_master_purple();
            ActionResult {
                action,
                success: ok,
                message: format!("PC purple scene: {msg}"),
            }
        }
        "rgb_brightness" => {
            let br = call
                .params
                .get("brightness")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as u8;
            let (ok, msg) = lighting::rgb_master_brightness(br);
            ActionResult {
                action,
                success: ok,
                message: format!("PC RGB {br}%: {msg}"),
            }
        }
        "light_purple" => {
            let result = lighting::lighting_master_purple();
            ActionResult {
                action,
                success: result.corsair_ok || result.openrgb_ok || result.govee_ok,
                message: format!(
                    "Purple scene: Corsair {} · OpenRGB {} · Govee {}",
                    if result.corsair_ok { "✓" } else { "✗" },
                    if result.openrgb_ok { "✓" } else { "✗" },
                    if result.govee_ok { "✓" } else { "✗" },
                ),
            }
        }
        "light_color" => {
            let r = call.params.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let g = call.params.get("g").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let b = call.params.get("b").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
            let result = lighting::lighting_master_color(r, g, b);
            ActionResult {
                action,
                success: result.corsair_ok || result.openrgb_ok || result.govee_ok,
                message: format!(
                    "Alle Lichter RGB ({r},{g},{b}): Corsair {} · OpenRGB {} · Govee {}",
                    if result.corsair_ok { "✓" } else { "✗" },
                    if result.openrgb_ok { "✓" } else { "✗" },
                    if result.govee_ok { "✓" } else { "✗" },
                ),
            }
        }
        "rgb_color" => {
            let r = call.params.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let g = call.params.get("g").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let b = call.params.get("b").and_then(|v| v.as_u64()).unwrap_or(255) as u8;
            let c_ok = corsair::corsair_set_rgb(corsair::SetRgbRequest { r, g, b }).is_ok();
            let _ = openrgb::openrgb_connect();
            let mut o_ok = 0u32;
            for id in OPENRGB_DEVICE_IDS {
                if openrgb::openrgb_set_color((*id).to_string(), r, g, b).is_ok() {
                    o_ok += 1;
                }
            }
            ActionResult {
                action,
                success: c_ok || o_ok > 0,
                message: format!(
                    "RGB ({r},{g},{b}): Corsair {} · {o_ok} OpenRGB",
                    if c_ok { "✓" } else { "✗" }
                ),
            }
        }
        "water_plant" => {
            let secs = call
                .params
                .get("seconds")
                .and_then(|v| v.as_u64())
                .unwrap_or(10)
                .min(60) as u32;
            let mut ok = 0u32;
            let mut fail = 0u32;
            for relay_id in 1..=6u32 {
                let relay_time = if relay_id == 2 || relay_id == 6 {
                    (secs as f32 * 0.7).round().max(1.0) as u32
                } else {
                    secs
                };
                let child = Command::new("curl")
                    .args([
                        "-sf",
                        "--max-time",
                        "10",
                        "-H",
                        "Content-Type: application/json",
                        "-d",
                        &format!(r#"{{"relay_id":{relay_id},"time":{relay_time}}}"#),
                        "http://192.168.0.21:8001/api/relay",
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();
                match child {
                    Ok(c) => match c.wait_with_output() {
                        Ok(o) if o.status.success() => ok += 1,
                        _ => fail += 1,
                    },
                    Err(_) => fail += 1,
                }
            }
            ActionResult {
                action,
                success: ok > 0,
                message: if fail == 0 {
                    format!("Alle 6 Pflanzen werden {secs}s gegossen (R2/R6: 70%) 💧")
                } else {
                    format!("{ok}/6 Relays ok, {fail} Fehler ({secs}s)")
                },
            }
        }
        "fan_speed" => {
            let percent = call.params.get("percent");
            let is_auto = percent
                .and_then(|v| v.as_str())
                .map(|s| s == "auto")
                .unwrap_or(false);
            let mut ok = 0u32;
            for ch in 0..6u8 {
                let speed_json = if is_auto {
                    serde_json::json!({"channel": ch})
                } else {
                    let pct = percent.and_then(|v| v.as_u64()).unwrap_or(50);
                    serde_json::json!({"channel": ch, "speed": pct})
                };
                if let Ok(req) = serde_json::from_value::<corsair::SetFanSpeedRequest>(speed_json) {
                    if corsair::corsair_set_fan_speed(req).is_ok() {
                        ok += 1;
                    }
                }
            }
            if is_auto {
                let _ = corsair::corsair_apply_fan_curves();
            }
            ActionResult {
                action,
                success: ok > 0,
                message: if is_auto {
                    format!("Fans back to auto curve ({ok}/6)")
                } else {
                    let pct = percent.and_then(|v| v.as_u64()).unwrap_or(50);
                    format!("Fans set to {pct}% ({ok}/6)")
                },
            }
        }
        "rgb_mode" => {
            let mode_name = call
                .params
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("static");
            let mode_id: usize = match mode_name.to_lowercase().as_str() {
                "off" => 0,
                "static" => 1,
                "pulse" => 2,
                "blink" | "blinking" => 3,
                "cycle" | "colorcycle" | "color_cycle" => 4,
                "wave" => 5,
                "random" => 6,
                _ => 1,
            };
            let speed = call
                .params
                .get("speed")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32);
            let _ = openrgb::openrgb_connect();
            let mut ok = 0u32;
            for id in OPENRGB_DEVICE_IDS {
                if openrgb::openrgb_set_mode((*id).to_string(), mode_id, speed, None, None, None)
                    .is_ok()
                {
                    ok += 1;
                }
            }
            ActionResult {
                action,
                success: ok > 0,
                message: format!("RGB mode '{mode_name}': {ok}/6 devices"),
            }
        }
        "snapshot_create" => {
            let desc = call
                .params
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("Jarvis snapshot")
                .to_string();
            match block_on_async(snapshots::create_snapshot("root".into(), desc.clone())) {
                Ok(r) => ActionResult {
                    action,
                    success: r.success,
                    message: format!("Snapshot created: {desc}"),
                },
                Err(e) => ActionResult {
                    action,
                    success: false,
                    message: format!("Snapshot error: {e}"),
                },
            }
        }
        "pi_reboot" => {
            let target = call
                .params
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("pi5");
            let devices = block_on_async(pi_remote::get_pi_devices()).unwrap_or_default();
            let device = devices.iter().find(|d| {
                let target_lower = target.to_lowercase();
                d.id.to_lowercase().contains(&target_lower)
                    || d.label.to_lowercase().contains(&target_lower)
                    || d.model.to_lowercase().contains(&target_lower)
            });
            match device {
                Some(d) => match block_on_async(pi_remote::pi_reboot(d.id.clone())) {
                    Ok(r) => ActionResult {
                        action,
                        success: r.success,
                        message: format!("{} rebooting", d.label),
                    },
                    Err(e) => ActionResult {
                        action,
                        success: false,
                        message: format!("Reboot error: {e}"),
                    },
                },
                None => ActionResult {
                    action,
                    success: false,
                    message: format!("Pi '{target}' not found"),
                },
            }
        }
        "music_stop" => {
            stop_music_playback();
            ActionResult {
                action,
                success: true,
                message: "Audio playback stopped".into(),
            }
        }
        "system_info" => match block_on_async(tuning::get_gpu_oc_status()) {
            Ok(gpu) => ActionResult {
                action,
                success: true,
                message: format!(
                    "GPU {}: {:.0}°C edge, {:.0}°C junction, {:.0}°C memory. \
                         Clock {} MHz (max {}). VRAM {} MHz. \
                         Power {}W of {}W cap. Fan {} RPM ({}). Load {}%.",
                    gpu.gpu_name,
                    gpu.temps.edge,
                    gpu.temps.junction,
                    gpu.temps.mem,
                    gpu.clocks.current_sclk_mhz,
                    gpu.clocks.sclk_max,
                    gpu.clocks.current_mclk_mhz,
                    gpu.power.current_w,
                    gpu.power.cap_w,
                    gpu.fan.rpm,
                    gpu.fan.mode,
                    gpu.gpu_busy_percent
                ),
            },
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("GPU info error: {e}"),
            },
        },
        "fan_status" => match corsair::corsair_ccxt_poll() {
            Ok(status) => {
                let val = serde_json::to_value(&status).unwrap_or_default();
                let mut parts = Vec::new();
                if let Some(fans) = val["fans"].as_array() {
                    for f in fans {
                        if f["connected"].as_bool().unwrap_or(false) {
                            let ch = f["channel"].as_u64().unwrap_or(0);
                            let rpm = f["rpm"].as_i64().unwrap_or(0);
                            let duty = f["duty"].as_u64().unwrap_or(0);
                            parts.push(format!("Fan {ch}: {rpm} RPM ({duty}%)"));
                        }
                    }
                }
                if let Some(temps) = val["temps"].as_array() {
                    for t in temps {
                        if t["connected"].as_bool().unwrap_or(false) {
                            let ch = t["channel"].as_u64().unwrap_or(0);
                            let temp = t["temp"].as_f64().unwrap_or(0.0);
                            parts.push(format!("Probe {ch}: {temp:.1}°C"));
                        }
                    }
                }
                ActionResult {
                    action,
                    success: true,
                    message: if parts.is_empty() {
                        "No fan data available".into()
                    } else {
                        parts.join(". ")
                    },
                }
            }
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("Fan status error: {e}"),
            },
        },
        "pi_status" => match block_on_async(pi_remote::get_pi_status_all()) {
            Ok(pis) => {
                let parts: Vec<String> = pis
                    .iter()
                    .map(|p| {
                        if p.online {
                            let temp = p
                                .cpu_temp
                                .map(|t| format!("{t:.0}°C"))
                                .unwrap_or_else(|| "n/a".into());
                            let cpu = p
                                .cpu_usage
                                .map(|u| format!("{u:.0}%"))
                                .unwrap_or_else(|| "n/a".into());
                            let mem = match (p.mem_used_mb, p.mem_total_mb) {
                                (Some(u), Some(t)) => format!("{u}/{t} MB"),
                                _ => "n/a".into(),
                            };
                            let up = p.uptime.as_deref().unwrap_or("n/a");
                            format!("{}: {temp}, CPU {cpu}, RAM {mem}, uptime {up}", p.label)
                        } else {
                            format!("{}: offline", p.label)
                        }
                    })
                    .collect();
                ActionResult {
                    action,
                    success: true,
                    message: if parts.is_empty() {
                        "No Pi devices configured".into()
                    } else {
                        parts.join(". ")
                    },
                }
            }
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("Pi status error: {e}"),
            },
        },
        "snapshot_list" => match block_on_async(snapshots::get_snapshots("root".into())) {
            Ok(snaps) => {
                let count = snaps.len();
                let last = snaps
                    .last()
                    .map(|s| {
                        let v = serde_json::to_value(s).unwrap_or_default();
                        let date = v["date"].as_str().unwrap_or("unknown");
                        let desc = v["description"].as_str().unwrap_or("");
                        let id = v["id"].as_u64().unwrap_or(0);
                        if desc.is_empty() {
                            format!("#{id} at {date}")
                        } else {
                            format!("#{id} at {date} ({desc})")
                        }
                    })
                    .unwrap_or_else(|| "none".into());
                ActionResult {
                    action,
                    success: true,
                    message: format!("{count} snapshots. Latest: {last}"),
                }
            }
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("Snapshot list error: {e}"),
            },
        },
        "timer_info" => match block_on_async(timer::get_timer_config()) {
            Ok(tc) => {
                let v = serde_json::to_value(&tc).unwrap_or_default();
                let enabled = v["enabled"].as_bool().unwrap_or(false);
                let calendar = v["calendar"].as_str().unwrap_or("not set");
                let last = v["last_trigger"].as_str().unwrap_or("never");
                let result = v["service_result"].as_str().unwrap_or("unknown");
                ActionResult {
                    action,
                    success: true,
                    message: format!(
                        "Backup timer: {}. Schedule: {calendar}. Last run: {last}. Result: {result}.",
                        if enabled { "enabled" } else { "disabled" }
                    ),
                }
            }
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("Timer info error: {e}"),
            },
        },
        "tent_status" => {
            let ts = block_on_async(pi_tent::get_pi_tent_status());
            let mut parts = Vec::new();
            if let Some(sensor) = &ts.sensor {
                parts.push(format!(
                    "Tent: {:.1}°C, {:.1}% humidity, VPD {:.2}, battery {}%",
                    sensor.temp, sensor.humi, sensor.vpd, sensor.batt
                ));
            }
            if let Some(light) = &ts.light {
                parts.push(format!(
                    "Light: {} ({}%)",
                    if light.power { "on" } else { "off" },
                    light.brightness
                ));
            }
            if let Some(tank) = &ts.tank {
                parts.push(format!(
                    "Tank: {:.0}% ({:.1} liters)",
                    tank.percent, tank.liters
                ));
            }
            ActionResult {
                action,
                success: ts.ok,
                message: if parts.is_empty() {
                    ts.error.unwrap_or_else(|| "Tent offline".into())
                } else {
                    parts.join(". ")
                },
            }
        }
        "system_status" => match block_on_async(health::get_system_status()) {
            Ok(ss) => {
                let v = serde_json::to_value(&ss).unwrap_or_default();
                let hostname = v["hostname"].as_str().unwrap_or("unknown");
                let kernel = v["kernel"].as_str().unwrap_or("unknown");
                let uptime = v["uptime"].as_str().unwrap_or("unknown");
                let mut disk_info = Vec::new();
                if let Some(disks) = v["disks"].as_array() {
                    for d in disks {
                        let name = d["name"].as_str().unwrap_or("disk");
                        let used = d["use_percent"].as_str().unwrap_or("?");
                        disk_info.push(format!("{name}: {used}"));
                    }
                }
                ActionResult {
                    action,
                    success: true,
                    message: format!(
                        "Host: {hostname}. Kernel: {kernel}. Uptime: {uptime}. Disk usage: {}.",
                        if disk_info.is_empty() {
                            "n/a".into()
                        } else {
                            disk_info.join(", ")
                        }
                    ),
                }
            }
            Err(e) => ActionResult {
                action,
                success: false,
                message: format!("System status error: {e}"),
            },
        },
        other => ActionResult {
            action: other.to_string(),
            success: false,
            message: format!("Unbekannte Aktion: {other}"),
        },
    }
}

fn parse_response(content: &str) -> (Option<Vec<ToolCall>>, String) {
    let trimmed = content.trim();
    let cleaned = if trimmed.starts_with("```") {
        trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
    } else {
        trimmed
    };

    if cleaned.starts_with('{') {
        if let Ok(jr) = serde_json::from_str::<JarvisLlmResponse>(cleaned) {
            let reply = jr.reply.unwrap_or_default();
            if !jr.actions.is_empty() {
                return (Some(jr.actions), reply);
            }
            if !reply.is_empty() {
                return (None, reply);
            }
        }
    }

    if let Some(start) = cleaned.find('{') {
        if let Some(end) = cleaned.rfind('}') {
            let slice = &cleaned[start..=end];
            if let Ok(jr) = serde_json::from_str::<JarvisLlmResponse>(slice) {
                let reply = jr.reply.unwrap_or_default();
                if !jr.actions.is_empty() {
                    return (Some(jr.actions), reply);
                }
                if !reply.is_empty() {
                    return (None, reply);
                }
            }
        }
    }

    (None, trimmed.to_string())
}

pub(crate) fn assistant_chat_sync(history: Vec<ChatMessage>) -> AssistantResponse {
    let prepared = match prepare_assistant_response(&history) {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };

    let results = execute_tool_calls(&prepared.calls);
    let text = compose_assistant_text(&prepared, &results);

    AssistantResponse {
        text,
        actions: results,
    }
}

pub(crate) fn assistant_status_sync() -> (bool, String) {
    match llm_http_client().get(LLAMA_HEALTH_URL).send() {
        Ok(response) if response.status().is_success() => (true, "Jarvis online".into()),
        Ok(_) => (false, "LLM Server nicht erreichbar".into()),
        Err(_) => (false, "LLM Server nicht erreichbar".into()),
    }
}

pub(crate) fn prepare_assistant_response(
    history: &[ChatMessage],
) -> Result<PreparedAssistantResponse, AssistantResponse> {
    if let Some(last_msg) = history.last() {
        if last_msg.role == "user" {
            if let Some((reply, kw_actions)) = intent::try_fast_parse(&last_msg.content) {
                let calls = kw_actions
                    .into_iter()
                    .map(|ka| ToolCall {
                        action: ka.action,
                        params: ka.params,
                    })
                    .collect();
                return Ok(PreparedAssistantResponse {
                    reply,
                    fallback_text: String::new(),
                    calls,
                });
            }
        }
    }

    let content = match call_llm(history) {
        Ok(c) => c,
        Err(e) => {
            return Err(AssistantResponse {
                text: format!("⚠️ {e}"),
                actions: vec![],
            });
        }
    };

    let (tool_calls, reply) = parse_response(&content);

    Ok(PreparedAssistantResponse {
        reply,
        fallback_text: content,
        calls: tool_calls.unwrap_or_default(),
    })
}

fn has_query_calls(calls: &[ToolCall]) -> bool {
    calls
        .iter()
        .any(|call| QUERY_ACTIONS.contains(&call.action.as_str()))
}

fn execute_tool_calls(calls: &[ToolCall]) -> Vec<ActionResult> {
    calls.iter().map(execute_tool).collect()
}

fn compose_assistant_text(
    prepared: &PreparedAssistantResponse,
    results: &[ActionResult],
) -> String {
    if prepared.calls.is_empty() {
        return if prepared.reply.is_empty() {
            prepared.fallback_text.clone()
        } else {
            prepared.reply.clone()
        };
    }

    let query_results: Vec<&str> = results
        .iter()
        .filter(|r| r.success && QUERY_ACTIONS.contains(&r.action.as_str()))
        .map(|r| r.message.as_str())
        .collect();

    if !query_results.is_empty() {
        query_results.join(". ")
    } else if !prepared.reply.is_empty() {
        prepared.reply.clone()
    } else {
        results
            .iter()
            .map(|r| {
                if r.success {
                    format!("✓ {}", r.message)
                } else {
                    format!("✗ {}", r.message)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn default_voice_ack(prepared: &PreparedAssistantResponse) -> String {
    if !prepared.reply.trim().is_empty() {
        prepared.reply.clone()
    } else {
        "Understood, Sir.".into()
    }
}

pub(crate) fn execute_voice_command(
    command: String,
    log_prefix: &str,
) -> (
    AssistantResponse,
    Option<std::thread::JoinHandle<Vec<ActionResult>>>,
) {
    let start = std::time::Instant::now();
    let history = vec![ChatMessage {
        role: "user".into(),
        content: command,
    }];

    let prepared = match prepare_assistant_response(&history) {
        Ok(prepared) => prepared,
        Err(response) => {
            log::warn!(
                "[{log_prefix}] Voice command failed before execution: {}",
                response.text
            );
            return (response, None);
        }
    };

    if prepared.calls.is_empty() || has_query_calls(&prepared.calls) {
        let results = execute_tool_calls(&prepared.calls);
        let text = compose_assistant_text(&prepared, &results);
        log::info!(
            "[{log_prefix}] Voice command resolved in {}ms: {}",
            start.elapsed().as_millis(),
            text
        );
        return (
            AssistantResponse {
                text,
                actions: results,
            },
            None,
        );
    }

    let ack = default_voice_ack(&prepared);
    let calls = prepared.calls.clone();
    let worker = std::thread::spawn(move || execute_tool_calls(&calls));

    log::info!(
        "[{log_prefix}] Starting {} mutation action(s) in parallel with acknowledgement",
        prepared.calls.len()
    );
    log::info!(
        "[{log_prefix}] Mutation command prepared in {}ms: {}",
        start.elapsed().as_millis(),
        ack
    );

    (
        AssistantResponse {
            text: ack,
            actions: vec![],
        },
        Some(worker),
    )
}
