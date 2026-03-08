use super::assistant::KeywordAction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    All,
    Govee,
    Rgb,
}

#[derive(Debug)]
struct ColorIntent {
    action: &'static str,
    reply: &'static str,
    params: serde_json::Value,
}

#[derive(Debug)]
struct ParsedSegment {
    scope: Option<Scope>,
    reply: String,
    actions: Vec<KeywordAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalColor {
    Blau,
    Lila,
    Orange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalCommand {
    RgbPower { on: bool },
    AllLightsPower { on: bool },
    AllLightsColor(CanonicalColor),
    GoveeBrightness { percent: u8 },
    FanSpeed { percent: u8 },
    TankLevel,
    GpuTemperature,
}

impl CanonicalCommand {
    fn canonical_phrase(self) -> String {
        match self {
            Self::RgbPower { on: true } => "rgb an".into(),
            Self::RgbPower { on: false } => "rgb aus".into(),
            Self::AllLightsPower { on: true } => "alle lichter an".into(),
            Self::AllLightsPower { on: false } => "alle lichter aus".into(),
            Self::AllLightsColor(CanonicalColor::Blau) => "alle lichter blau".into(),
            Self::AllLightsColor(CanonicalColor::Lila) => "alle lichter lila".into(),
            Self::AllLightsColor(CanonicalColor::Orange) => "alle lichter orange".into(),
            Self::GoveeBrightness { percent } => format!("govee auf {percent} prozent"),
            Self::FanSpeed { percent } => format!("luefter auf {percent} prozent"),
            Self::TankLevel => "wie voll ist der tank".into(),
            Self::GpuTemperature => "wie warm ist die gpu".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct VoiceCommandResolution {
    pub normalized: String,
    pub repaired: String,
    pub canonical: Option<String>,
    pub best_candidate: Option<String>,
    pub score: f32,
    pub threshold: f32,
    pub accepted: bool,
    pub reject_reason: Option<String>,
    pub allow_open_fallback: bool,
    pub command: Option<CanonicalCommand>,
}

#[derive(Debug, Clone)]
struct ResolverCandidate {
    command: CanonicalCommand,
    canonical: String,
    score: f32,
    threshold: f32,
}

struct ResolverSpec {
    canonical: &'static str,
    aliases: &'static [&'static str],
    required_groups: &'static [&'static [&'static str]],
    optional_tokens: &'static [&'static str],
    negative_tokens: &'static [&'static str],
    min_tokens: usize,
    threshold: f32,
}

pub(crate) fn try_fast_parse(input: &str) -> Option<(String, Vec<KeywordAction>)> {
    let normalized = normalize_input(input);
    let segments = split_segments(&normalized);
    let mut scope_hint = detect_scope(&normalized);
    let mut replies = Vec::new();
    let mut actions = Vec::new();

    for segment in segments {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(parsed) = parse_segment(trimmed, scope_hint) {
            if let Some(scope) = parsed.scope {
                scope_hint = Some(scope);
            }
            if !parsed.reply.is_empty() {
                replies.push(parsed.reply);
            }
            actions.extend(parsed.actions);
        }
    }

    if actions.is_empty() {
        None
    } else {
        Some((compose_reply(&replies), actions))
    }
}

pub(crate) fn canonicalize_voice_command(input: &str) -> String {
    let resolution = resolve_voice_command(input);
    resolution.canonical.unwrap_or(resolution.repaired)
}

pub(crate) fn canonicalize_input(input: &str) -> String {
    let mut s = normalize_input(input);
    if s.is_empty() {
        return s;
    }

    for (from, to) in [
        ("c gpu", "gpu"),
        ("cgpu", "gpu"),
        ("gpu temp", "gpu temperatur"),
        ("g p u", "gpu"),
        ("covi", "govee"),
        ("kovi", "govee"),
        ("govi", "govee"),
        ("go vee", "govee"),
        ("go wi", "govee"),
        ("r g b", "rgb"),
        ("egb", "rgb"),
        ("ergb", "rgb"),
        ("pro cent", "prozent"),
        ("pro zent", "prozent"),
    ] {
        s = s.replace(from, to);
    }

    collapse_spaces(&s)
}

pub(crate) fn repair_voice_input(input: &str) -> String {
    let s = canonicalize_input(input);
    if s.is_empty() {
        return s;
    }

    if matches!(
        s.trim(),
        "bildschirmherr" | "bildschirm herr" | "wie auf 50 prozent" | "haben sie gpu"
    ) {
        return String::new();
    }

    s
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn hard_reject(input: &str) -> bool {
    hard_reject_reason(input).is_some()
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_closed_set_command(input: &str) -> Option<CanonicalCommand> {
    resolve_voice_command(input).command
}

pub(crate) fn resolve_voice_command(input: &str) -> VoiceCommandResolution {
    let normalized = normalize_input(input);
    let repaired = repair_voice_input(input);
    let tokens = tokenize(&repaired);
    let reject_reason = hard_reject_reason(&repaired);
    let best_candidate = best_closed_set_candidate(&repaired, &tokens);

    let command = best_candidate.as_ref().and_then(|candidate| {
        if reject_reason.is_none() && candidate.score >= candidate.threshold {
            Some(candidate.command)
        } else {
            None
        }
    });

    let allow_open_fallback = reject_reason.is_none()
        && command.is_none()
        && tokens.len() >= 4
        && repaired.chars().count() >= 18;

    VoiceCommandResolution {
        normalized,
        repaired,
        canonical: command.map(CanonicalCommand::canonical_phrase),
        best_candidate: best_candidate.as_ref().map(|candidate| candidate.canonical.clone()),
        score: best_candidate.as_ref().map_or(0.0, |candidate| candidate.score),
        threshold: best_candidate.as_ref().map_or(0.0, |candidate| candidate.threshold),
        accepted: command.is_some(),
        reject_reason,
        allow_open_fallback,
        command,
    }
}

pub(crate) fn closed_set_command_actions(
    command: CanonicalCommand,
) -> (String, Vec<KeywordAction>) {
    match command {
        CanonicalCommand::RgbPower { on } => (
            if on {
                "PC lighting enabled.".into()
            } else {
                "PC lighting shutting down.".into()
            },
            vec![KeywordAction {
                action: "rgb_power".into(),
                params: serde_json::json!({"power": on}),
            }],
        ),
        CanonicalCommand::AllLightsPower { on } => (
            if on {
                "Right away, Sir. Bringing all lights online.".into()
            } else {
                "Certainly, Sir. Turning the lights off.".into()
            },
            vec![KeywordAction {
                action: "light_power".into(),
                params: serde_json::json!({"power": on}),
            }],
        ),
        CanonicalCommand::AllLightsColor(CanonicalColor::Blau) => (
            "Blue, immediately.".into(),
            vec![KeywordAction {
                action: "light_color".into(),
                params: serde_json::json!({"r": 0, "g": 0, "b": 255}),
            }],
        ),
        CanonicalCommand::AllLightsColor(CanonicalColor::Lila) => (
            "Purple, across the full lighting grid.".into(),
            vec![KeywordAction {
                action: "light_purple".into(),
                params: serde_json::json!({}),
            }],
        ),
        CanonicalCommand::AllLightsColor(CanonicalColor::Orange) => (
            "Orange. Applied across the lighting grid.".into(),
            vec![KeywordAction {
                action: "light_color".into(),
                params: serde_json::json!({"r": 255, "g": 140, "b": 0}),
            }],
        ),
        CanonicalCommand::GoveeBrightness { percent } => (
            format!("Setting Govee brightness to {percent} percent."),
            vec![KeywordAction {
                action: "govee_brightness".into(),
                params: serde_json::json!({"brightness": percent}),
            }],
        ),
        CanonicalCommand::FanSpeed { percent } => (
            format!("Setting all fans to {percent} percent, Sir."),
            vec![KeywordAction {
                action: "fan_speed".into(),
                params: serde_json::json!({"percent": percent}),
            }],
        ),
        CanonicalCommand::TankLevel => (
            "Checking the tank.".into(),
            vec![KeywordAction {
                action: "tent_status".into(),
                params: serde_json::json!({"mode":"tank"}),
            }],
        ),
        CanonicalCommand::GpuTemperature => (
            "Checking GPU status.".into(),
            vec![KeywordAction {
                action: "system_info".into(),
                params: serde_json::json!({}),
            }],
        ),
    }
}

fn parse_segment(segment: &str, inherited_scope: Option<Scope>) -> Option<ParsedSegment> {
    if let Some(parsed) = parse_query(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_screen_brightness(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_watering(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_fan(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_desktop_action(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_volume(segment) {
        return Some(parsed);
    }

    let scope = detect_scope(segment).or(inherited_scope);
    let mut actions = Vec::new();
    let mut reply_parts = Vec::new();

    if let Some(power) = extract_power(segment) {
        let scope = scope.unwrap_or(Scope::All);
        actions.push(power_action(scope, power));
        reply_parts.push(power_reply(scope, power));
    }

    if let Some(color) = extract_color_intent(segment, scope) {
        actions.push(KeywordAction {
            action: color.action.into(),
            params: color.params,
        });
        reply_parts.push(color.reply.to_string());
    }

    if let Some(brightness) = extract_brightness(segment) {
        let has_light_context = contains_any(
            segment,
            &[
                "licht",
                "lichter",
                "lampe",
                "lampen",
                "beleuchtung",
                "alles",
                "govee",
                "deckenlampe",
                "stehlampe",
                "rachel",
                "rgb",
                "pc",
                "corsair",
            ],
        );
        if scope.is_some() || has_light_context {
            let scope = scope.unwrap_or(Scope::All);
            actions.push(brightness_action(scope, brightness));
            reply_parts.push(brightness_reply(scope, brightness));
        }
    }

    if actions.is_empty() {
        None
    } else {
        Some(ParsedSegment {
            scope,
            reply: reply_parts.join(" "),
            actions,
        })
    }
}

/// Fast-parse screen/monitor brightness commands.
fn parse_screen_brightness(segment: &str) -> Option<ParsedSegment> {
    let is_screen = contains_any(
        segment,
        &[
            "bildschirm",
            "monitor",
            "screen",
            "display",
            "bildschirmhelligkeit",
            "monitorhelligkeit",
        ],
    );
    if !is_screen {
        return None;
    }

    // "Bildschirm aus" / "Monitor aus"
    if contains_any(segment, &["aus", "off", "schwarz"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Turning screens off.".into(),
            actions: vec![KeywordAction {
                action: "screen_brightness".into(),
                params: serde_json::json!({"percent": 0}),
            }],
        });
    }

    // "Bildschirm an" / "Monitor voll" / "volle Helligkeit"
    if contains_any(segment, &["an", "on", "voll", "maximum", "max"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Setting screens to full brightness.".into(),
            actions: vec![KeywordAction {
                action: "screen_brightness".into(),
                params: serde_json::json!({"percent": 100}),
            }],
        });
    }

    // "Bildschirm heller" / "Monitor heller"
    if contains_any(segment, &["heller", "brighter"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Increasing screen brightness.".into(),
            actions: vec![KeywordAction {
                action: "screen_brightness".into(),
                params: serde_json::json!({"percent": 80}),
            }],
        });
    }

    // "Bildschirm dunkler" / "Monitor dunkler" / "dimmen"
    if contains_any(segment, &["dunkler", "dimmen", "dimmer", "darker"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Dimming screens.".into(),
            actions: vec![KeywordAction {
                action: "screen_brightness".into(),
                params: serde_json::json!({"percent": 30}),
            }],
        });
    }

    // "Bildschirm auf 50 Prozent"
    let words: Vec<&str> = segment.split_whitespace().collect();
    for word in &words {
        let cleaned = word
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim_end_matches("prozent");
        if let Ok(value) = cleaned.parse::<u16>() {
            if value <= 100 {
                return Some(ParsedSegment {
                    scope: None,
                    reply: format!("Setting screen brightness to {value} percent."),
                    actions: vec![KeywordAction {
                        action: "screen_brightness".into(),
                        params: serde_json::json!({"percent": value}),
                    }],
                });
            }
        }
    }

    None
}

/// Fast-parse common query commands so they bypass the LLM entirely.
/// This also catches frequent Whisper misrecognitions (e.g. "Eistatus" for "Pi Status").
fn parse_query(segment: &str) -> Option<ParsedSegment> {
    // Pi Status (+ Whisper misrecognitions: "eistatus", "feinstatus", "pistatus", etc.)
    if contains_any(
        segment,
        &[
            "pi status",
            "pistatus",
            "eistatus",
            "eis status",
            "feinstatus",
            "fein status",
            "heilsstatus",
            "pallstatus",
            "pie status",
            "pi 4 status",
            "pi 5 status",
            "raspberry pi status",
            "wie geht es dem pi",
            "wie geht es den pis",
            "pi temperatur",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking the Pis.".into(),
            actions: vec![KeywordAction {
                action: "pi_status".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // System Status
    if contains_any(
        segment,
        &[
            "system status",
            "systemstatus",
            "system info",
            "systeminfo",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking system status.".into(),
            actions: vec![KeywordAction {
                action: "system_status".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // GPU Info
    if contains_any(
        segment,
        &[
            "gpu temperatur",
            "gpu temp",
            "wie warm ist die gpu",
            "wie warmsige gpu",
            "gpu info",
            "vram",
            "wie viel vram",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking GPU status.".into(),
            actions: vec![KeywordAction {
                action: "system_info".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // Tent Status
    if contains_any(
        segment,
        &[
            "zelt status",
            "zeltstatus",
            "wie ist das zelt",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking the tent.".into(),
            actions: vec![KeywordAction {
                action: "tent_status".into(),
                params: serde_json::json!({"mode":"full"}),
            }],
        });
    }

    if contains_any(segment, &["tankinhalt", "wie voll ist der tank", "tank"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking the tank.".into(),
            actions: vec![KeywordAction {
                action: "tent_status".into(),
                params: serde_json::json!({"mode":"tank"}),
            }],
        });
    }

    if contains_any(
        segment,
        &[
            "wie warm ist das zelt",
            "zelt temperatur",
            "zelt luftfeuchtigkeit",
            "zelt klima",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking tent climate.".into(),
            actions: vec![KeywordAction {
                action: "tent_status".into(),
                params: serde_json::json!({"mode":"climate"}),
            }],
        });
    }

    // Fan Status
    if contains_any(
        segment,
        &[
            "wie geht es den lueftern",
            "luefterstatus",
            "luefter status",
            "fan status",
            "wassertemperatur",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking fans.".into(),
            actions: vec![KeywordAction {
                action: "fan_status".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // Snapshot List
    if contains_any(
        segment,
        &[
            "snapshot liste",
            "zeig die snapshots",
            "wie viele snapshots",
            "snapshot uebersicht",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking snapshots.".into(),
            actions: vec![KeywordAction {
                action: "snapshot_list".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    None
}

/// Fast-parse desktop management commands: launch app, screenshot, lock, power, audio.
fn parse_desktop_action(segment: &str) -> Option<ParsedSegment> {
    // Launch app: "öffne firefox", "starte steam", "mach firefox auf", "open terminal"
    let app_triggers = [
        ("firefox", "firefox", "Opening Firefox."),
        ("browser", "firefox", "Opening the browser."),
        ("steam", "steam", "Launching Steam."),
        ("code", "code", "Opening VS Code."),
        ("vscode", "code", "Opening VS Code."),
        ("vs code", "code", "Opening VS Code."),
        ("terminal", "terminal", "Opening a terminal."),
        ("konsole", "terminal", "Opening a terminal."),
        ("thunar", "thunar", "Opening Thunar."),
        ("dateimanager", "thunar", "Opening Thunar."),
        ("dateien", "thunar", "Opening Thunar."),
        ("spotify", "spotify", "Opening Spotify."),
        ("discord", "discord", "Opening Discord."),
        ("obsidian", "obsidian", "Opening Obsidian."),
        ("gimp", "gimp", "Opening GIMP."),
        ("vlc", "vlc", "Opening VLC."),
        ("lutris", "lutris", "Opening Lutris."),
    ];

    if contains_any(segment, &["oeffne", "starte", "open", "start", "launch", "mach"]) {
        for (keyword, app, reply) in &app_triggers {
            if segment.contains(keyword) {
                return Some(ParsedSegment {
                    scope: None,
                    reply: reply.to_string(),
                    actions: vec![KeywordAction {
                        action: "launch_app".into(),
                        params: serde_json::json!({"app": app}),
                    }],
                });
            }
        }
    }

    // Screenshot
    if contains_any(segment, &["screenshot", "bildschirmfoto", "screen capture", "screenschat"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Screenshot taken.".into(),
            actions: vec![KeywordAction {
                action: "screenshot".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // Lock screen
    if contains_any(segment, &[
        "sperre den bildschirm",
        "bildschirm sperren",
        "lock screen",
        "sperren",
        "lock",
    ]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Locking screen.".into(),
            actions: vec![KeywordAction {
                action: "lock_screen".into(),
                params: serde_json::json!({}),
            }],
        });
    }

    // System power
    if contains_any(segment, &["herunterfahren", "runterfahren", "shutdown", "shut down", "fahr runter", "fahr den pc runter", "pc aus"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Shutting down. Goodnight, Sir.".into(),
            actions: vec![KeywordAction {
                action: "system_power".into(),
                params: serde_json::json!({"mode": "shutdown"}),
            }],
        });
    }

    if contains_any(segment, &["neustart", "reboot", "neu starten", "pc neu starten"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Rebooting now.".into(),
            actions: vec![KeywordAction {
                action: "system_power".into(),
                params: serde_json::json!({"mode": "reboot"}),
            }],
        });
    }

    if contains_any(segment, &["suspend", "schlafmodus", "standby", "schlafen"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Suspending.".into(),
            actions: vec![KeywordAction {
                action: "system_power".into(),
                params: serde_json::json!({"mode": "suspend"}),
            }],
        });
    }

    None
}

/// Fast-parse volume commands: "lautstärke auf 50", "leiser", "lauter", "mute"
fn parse_volume(segment: &str) -> Option<ParsedSegment> {
    let is_volume = contains_any(segment, &[
        "lautstaerke", "volume", "leiser", "lauter", "mute", "stumm", "ton",
    ]);
    if !is_volume {
        return None;
    }

    // Mute
    if contains_any(segment, &["mute", "stumm", "ton aus"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Muted.".into(),
            actions: vec![KeywordAction {
                action: "volume".into(),
                params: serde_json::json!({"percent": 0}),
            }],
        });
    }

    // Louder
    if contains_any(segment, &["lauter", "louder"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Turning volume up.".into(),
            actions: vec![KeywordAction {
                action: "volume".into(),
                params: serde_json::json!({"percent": 70}),
            }],
        });
    }

    // Quieter
    if contains_any(segment, &["leiser", "quieter"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Turning volume down.".into(),
            actions: vec![KeywordAction {
                action: "volume".into(),
                params: serde_json::json!({"percent": 30}),
            }],
        });
    }

    // "Lautstärke auf 50 Prozent"
    let words: Vec<&str> = segment.split_whitespace().collect();
    for word in &words {
        let cleaned = word
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim_end_matches("prozent");
        if let Ok(value) = cleaned.parse::<u16>() {
            if value <= 150 {
                return Some(ParsedSegment {
                    scope: None,
                    reply: format!("Volume set to {value} percent."),
                    actions: vec![KeywordAction {
                        action: "volume".into(),
                        params: serde_json::json!({"percent": value}),
                    }],
                });
            }
        }
    }

    None
}

fn parse_watering(segment: &str) -> Option<ParsedSegment> {
    if contains_any(
        segment,
        &[
            "pflanze giessen",
            "pflanzen giessen",
            "giess die pflanze",
            "giesse die pflanze",
            "pflanze waessern",
            "pflanzen waessern",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Watering the plants now, Sir.".into(),
            actions: vec![KeywordAction {
                action: "water_plant".into(),
                params: serde_json::json!({"seconds": 10}),
            }],
        });
    }

    if contains_any(segment, &["kurz giessen", "kurz waessern"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Short watering cycle engaged.".into(),
            actions: vec![KeywordAction {
                action: "water_plant".into(),
                params: serde_json::json!({"seconds": 5}),
            }],
        });
    }

    if contains_any(
        segment,
        &["viel giessen", "ordentlich giessen", "lange giessen"],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Generous watering cycle engaged.".into(),
            actions: vec![KeywordAction {
                action: "water_plant".into(),
                params: serde_json::json!({"seconds": 20}),
            }],
        });
    }

    None
}

fn parse_fan(segment: &str) -> Option<ParsedSegment> {
    let is_fan = contains_any(
        segment,
        &[
            "luefter",
            "fan",
            "fans",
            "geblaese",
            "ventilator",
        ],
    );
    if !is_fan {
        return None;
    }

    // "Lüfter auto" / "Lüfter automatisch" / "Lüfter zurücksetzen"
    if contains_any(
        segment,
        &["auto", "automatisch", "zuruecksetzen", "normal", "reset", "kurve"],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Returning fans to automatic curve, Sir.".into(),
            actions: vec![KeywordAction {
                action: "fan_speed".into(),
                params: serde_json::json!({"percent": "auto"}),
            }],
        });
    }

    // "Lüfter auf 80 Prozent" / "Lüfter 50" / "Fans auf maximum"
    if contains_any(segment, &["maximum", "max", "voll"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Setting all fans to maximum, Sir.".into(),
            actions: vec![KeywordAction {
                action: "fan_speed".into(),
                params: serde_json::json!({"percent": 100}),
            }],
        });
    }

    if contains_any(segment, &["minimum", "min", "leise", "silent", "quiet"]) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Setting fans to minimum speed.".into(),
            actions: vec![KeywordAction {
                action: "fan_speed".into(),
                params: serde_json::json!({"percent": 20}),
            }],
        });
    }

    // Extract numeric percent value
    let words: Vec<&str> = segment.split_whitespace().collect();
    for word in &words {
        let cleaned = word
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim_end_matches("prozent");
        if let Ok(value) = cleaned.parse::<u16>() {
            if value > 0 && value <= 100 {
                return Some(ParsedSegment {
                    scope: None,
                    reply: format!("Setting all fans to {value} percent, Sir."),
                    actions: vec![KeywordAction {
                        action: "fan_speed".into(),
                        params: serde_json::json!({"percent": value}),
                    }],
                });
            }
        }
    }

    None
}

fn detect_scope(input: &str) -> Option<Scope> {
    if contains_any(
        input,
        &[
            "alles",
            "alle lichter",
            "alle lampen",
            "komplette beleuchtung",
            "gesamte beleuchtung",
        ],
    ) {
        Some(Scope::All)
    } else if contains_any(
        input,
        &[
            "govee",
            "decke",
            "deckenlampe",
            "deckenlampen",
            "stehlampe",
            "stehlampen",
            "rachel",
        ],
    ) {
        Some(Scope::Govee)
    } else if contains_any(
        input,
        &[
            "rgb",
            "pc",
            "computer",
            "rechner",
            "corsair",
            "keyboard",
            "tastatur",
            "maus",
            "mousepad",
            "mainboard",
        ],
    ) {
        Some(Scope::Rgb)
    } else if contains_any(
        input,
        &[
            "alle lichter",
            "lichter",
            "alle lampen",
            "lampen",
            "licht",
            "beleuchtung",
        ],
    ) {
        Some(Scope::All)
    } else {
        None
    }
}

fn has_explicit_all_scope(input: &str) -> bool {
    contains_any(
        input,
        &[
            "alles",
            "alle lichter",
            "alle lampen",
            "komplette beleuchtung",
            "gesamte beleuchtung",
            "lichter",
        ],
    )
}

fn extract_power(input: &str) -> Option<bool> {
    if contains_any(
        input,
        &[
            "licht aus",
            "lichter aus",
            "lampen aus",
            "alle lichter aus",
            "alle lampen aus",
            "alles aus",
            "govee aus",
            "rgb aus",
            "pc rgb aus",
            "pc licht aus",
            "ausschalten",
            "abschalten",
            "ausmachen",
            "offline",
            "dunkel machen",
        ],
    ) {
        Some(false)
    } else if contains_any(
        input,
        &[
            "licht an",
            "lichter an",
            "lampen an",
            "alle lichter an",
            "alle lampen an",
            "alles an",
            "govee an",
            "rgb an",
            "pc rgb an",
            "pc licht an",
            "einschalten",
            "anschalten",
            "anmachen",
            "online",
        ],
    ) {
        Some(true)
    } else {
        None
    }
}

fn extract_color_intent(input: &str, scope: Option<Scope>) -> Option<ColorIntent> {
    let scope = if has_explicit_all_scope(input) {
        Scope::All
    } else {
        scope.unwrap_or(Scope::All)
    };
    let single_lamp = detect_govee_lamp(input);

    if contains_any(input, &["lila", "lilan", "violett", "purple", "magenta"]) {
        return Some(match scope {
            Scope::All => ColorIntent {
                action: "light_purple",
                reply: "Purple, across the full lighting grid.",
                params: serde_json::json!({}),
            },
            Scope::Govee => ColorIntent {
                action: if single_lamp.is_some() {
                    "govee_lamp_color"
                } else {
                    "govee_purple"
                },
                reply: if let Some(lamp) = single_lamp {
                    match lamp {
                        "deckenlampe" => "Applying purple to the ceiling lamp.",
                        "deckenlampe2" => "Applying purple to ceiling lamp two.",
                        "stehlampe" => "Applying purple to the floor lamp.",
                        "rachel" => "Applying purple to Rachel.",
                        _ => "Applying the purple scene to Govee.",
                    }
                } else {
                    "Applying the purple scene to Govee."
                },
                params: if let Some(lamp) = single_lamp {
                    serde_json::json!({"lamp": lamp, "r": 128, "g": 0, "b": 255})
                } else {
                    serde_json::json!({})
                },
            },
            Scope::Rgb => ColorIntent {
                action: "rgb_purple",
                reply: "Applying the purple scene to PC RGB.",
                params: serde_json::json!({}),
            },
        });
    }

    let rgb = if contains_any(input, &["rot", "red"]) {
        Some((255, 0, 0, "Red. Excellent choice, Sir."))
    } else if contains_any(input, &["blau", "blue"]) {
        Some((0, 0, 255, "Blue, immediately."))
    } else if contains_any(input, &["gruen", "grun", "green"]) {
        Some((0, 255, 0, "Green. Done."))
    } else if contains_any(input, &["orange"]) {
        Some((255, 140, 0, "Orange. Applied across the lighting grid."))
    } else if contains_any(input, &["weiss", "white"]) {
        Some((255, 255, 255, "White. Clean, Sir."))
    } else {
        None
    }?;

    let action = match scope {
        Scope::All => "light_color",
        Scope::Govee => {
            if single_lamp.is_some() {
                "govee_lamp_color"
            } else {
                "govee_color"
            }
        }
        Scope::Rgb => "rgb_color",
    };

    Some(ColorIntent {
        action,
        reply: match single_lamp {
            Some("deckenlampe") => "Setting the ceiling lamp color.",
            Some("deckenlampe2") => "Setting ceiling lamp two.",
            Some("stehlampe") => "Setting the floor lamp color.",
            Some("rachel") => "Setting Rachel's color.",
            _ => rgb.3,
        },
        params: if let Some(lamp) = single_lamp {
            serde_json::json!({"lamp": lamp, "r": rgb.0, "g": rgb.1, "b": rgb.2})
        } else {
            serde_json::json!({"r": rgb.0, "g": rgb.1, "b": rgb.2})
        },
    })
}

fn detect_govee_lamp(input: &str) -> Option<&'static str> {
    if contains_any(input, &["rachel"]) {
        Some("rachel")
    } else if contains_any(input, &["stehlampe", "bodenlampe"]) {
        Some("stehlampe")
    } else if contains_any(
        input,
        &["deckenlampe 2", "deckenlampe2", "zweite deckenlampe", "decke 2"],
    ) {
        Some("deckenlampe2")
    } else if contains_any(input, &["deckenlampe", "decke", "deckenlicht"]) {
        Some("deckenlampe")
    } else {
        None
    }
}

fn extract_brightness(input: &str) -> Option<u8> {
    // Don't match brightness for fan-related commands
    if contains_any(
        input,
        &["luefter", "fan", "fans", "geblase", "geblaese", "ventilator"],
    ) {
        return None;
    }

    let words: Vec<&str> = input.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let cleaned = word
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim_end_matches("prozent");
        let Ok(value) = cleaned.parse::<u16>() else {
            continue;
        };
        if value == 0 || value > 100 {
            continue;
        }

        let start = index.saturating_sub(2);
        let end = usize::min(index + 2, words.len().saturating_sub(1));
        let context = words[start..=end].join(" ");
        if contains_any(
            &context,
            &[
                "%",
                "prozent",
                "helligkeit",
                "auf",
                "heller",
                "dunkler",
                "brightness",
            ],
        ) {
            return u8::try_from(value).ok();
        }
    }
    None
}

fn power_action(scope: Scope, power: bool) -> KeywordAction {
    let action = match scope {
        Scope::All => "light_power",
        Scope::Govee => "govee_power",
        Scope::Rgb => "rgb_power",
    };
    KeywordAction {
        action: action.into(),
        params: serde_json::json!({"power": power}),
    }
}

fn brightness_action(scope: Scope, brightness: u8) -> KeywordAction {
    let action = match scope {
        Scope::All => "light_brightness",
        Scope::Govee => "govee_brightness",
        Scope::Rgb => "rgb_brightness",
    };
    KeywordAction {
        action: action.into(),
        params: serde_json::json!({"brightness": brightness}),
    }
}

fn power_reply(scope: Scope, power: bool) -> String {
    match (scope, power) {
        (Scope::All, true) => "Right away, Sir. Bringing all lights online.".into(),
        (Scope::All, false) => "Certainly, Sir. Turning the lights off.".into(),
        (Scope::Govee, true) => "Govee lights online, Sir.".into(),
        (Scope::Govee, false) => "Govee lights going offline.".into(),
        (Scope::Rgb, true) => "PC lighting enabled.".into(),
        (Scope::Rgb, false) => "PC lighting shutting down.".into(),
    }
}

fn brightness_reply(scope: Scope, brightness: u8) -> String {
    match scope {
        Scope::All => format!("Brightness set to {brightness} percent, Sir."),
        Scope::Govee => format!("Setting Govee brightness to {brightness} percent."),
        Scope::Rgb => format!("Setting PC RGB brightness to {brightness} percent."),
    }
}

fn best_closed_set_candidate(input: &str, tokens: &[String]) -> Option<ResolverCandidate> {
    let mut candidates = vec![
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::RgbPower { on: false },
            ResolverSpec {
                canonical: "rgb aus",
                aliases: &["rgb aus", "pc rgb aus", "pc licht aus"],
                required_groups: &[&["rgb"], &["aus", "off", "ausschalten", "abschalten"]],
                optional_tokens: &["pc", "licht"],
                negative_tokens: &["govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.78,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::RgbPower { on: true },
            ResolverSpec {
                canonical: "rgb an",
                aliases: &["rgb an", "pc rgb an", "pc licht an"],
                required_groups: &[&["rgb"], &["an", "on", "einschalten", "anmachen", "anschalten"]],
                optional_tokens: &["pc", "licht"],
                negative_tokens: &["govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.78,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::AllLightsPower { on: false },
            ResolverSpec {
                canonical: "alle lichter aus",
                aliases: &["alle lichter aus", "lichter aus", "licht aus", "alle lampen aus"],
                required_groups: &[
                    &["licht", "lichter", "lampe", "lampen", "beleuchtung"],
                    &["aus", "off", "ausschalten", "abschalten", "ausmachen"],
                ],
                optional_tokens: &["alle", "alles"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.8,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::AllLightsPower { on: true },
            ResolverSpec {
                canonical: "alle lichter an",
                aliases: &["alle lichter an", "lichter an", "licht an", "alle lampen an"],
                required_groups: &[
                    &["licht", "lichter", "lampe", "lampen", "beleuchtung"],
                    &["an", "on", "einschalten", "anmachen", "anschalten"],
                ],
                optional_tokens: &["alle", "alles"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.8,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::AllLightsColor(CanonicalColor::Blau),
            ResolverSpec {
                canonical: "alle lichter blau",
                aliases: &["alle lichter blau", "lichter blau", "licht blau"],
                required_groups: &[
                    &["licht", "lichter", "lampe", "lampen", "beleuchtung"],
                    &["blau", "blue"],
                ],
                optional_tokens: &["alle"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.84,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::AllLightsColor(CanonicalColor::Lila),
            ResolverSpec {
                canonical: "alle lichter lila",
                aliases: &["alle lichter lila", "alle lichter violett", "lichter lila"],
                required_groups: &[
                    &["licht", "lichter", "lampe", "lampen", "beleuchtung"],
                    &["lila", "violett", "purple", "magenta"],
                ],
                optional_tokens: &["alle"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.84,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::AllLightsColor(CanonicalColor::Orange),
            ResolverSpec {
                canonical: "alle lichter orange",
                aliases: &["alle lichter orange", "lichter orange", "licht orange"],
                required_groups: &[
                    &["licht", "lichter", "lampe", "lampen", "beleuchtung"],
                    &["orange"],
                ],
                optional_tokens: &["alle"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "gpu", "prozent"],
                min_tokens: 2,
                threshold: 0.84,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::TankLevel,
            ResolverSpec {
                canonical: "wie voll ist der tank",
                aliases: &["wie voll ist der tank", "tankinhalt", "tank fuellstand"],
                required_groups: &[&["tank", "tankinhalt"], &["voll", "fuellstand", "inhalt"]],
                optional_tokens: &["wie", "ist", "der"],
                negative_tokens: &["rgb", "govee", "luefter", "gpu"],
                min_tokens: 2,
                threshold: 0.82,
            },
        ),
        resolve_fixed_candidate(
            input,
            tokens,
            CanonicalCommand::GpuTemperature,
            ResolverSpec {
                canonical: "wie warm ist die gpu",
                aliases: &["wie warm ist die gpu", "gpu temperatur", "gpu temp"],
                required_groups: &[&["gpu"], &["warm", "temperatur", "temp"]],
                optional_tokens: &["wie", "ist", "die"],
                negative_tokens: &["rgb", "govee", "luefter", "tank", "prozent"],
                min_tokens: 2,
                threshold: 0.82,
            },
        ),
    ];

    candidates.push(resolve_percent_candidate(
        input,
        tokens,
        ResolverSpec {
            canonical: "govee auf prozent",
            aliases: &["govee auf", "govee helligkeit"],
            required_groups: &[&["govee"], &["auf", "prozent", "helligkeit"]],
            optional_tokens: &["prozent", "helligkeit"],
            negative_tokens: &["rgb", "luefter", "tank", "gpu"],
            min_tokens: 3,
            threshold: 0.83,
        },
        |percent| CanonicalCommand::GoveeBrightness { percent },
    ));

    candidates.push(resolve_percent_candidate(
        input,
        tokens,
        ResolverSpec {
            canonical: "luefter auf prozent",
            aliases: &["luefter auf", "fan speed", "fans auf"],
            required_groups: &[&["luefter", "fan", "fans", "ventilator"], &["auf", "prozent"]],
            optional_tokens: &["alle", "speed"],
            negative_tokens: &["rgb", "govee", "tank", "gpu"],
            min_tokens: 3,
            threshold: 0.83,
        },
        |percent| CanonicalCommand::FanSpeed { percent },
    ));

    candidates
        .into_iter()
        .flatten()
        .max_by(|left, right| left.score.total_cmp(&right.score))
}

fn resolve_fixed_candidate(
    input: &str,
    tokens: &[String],
    command: CanonicalCommand,
    spec: ResolverSpec,
) -> Option<ResolverCandidate> {
    let score = score_spec(input, tokens, &spec)?;
    Some(ResolverCandidate {
        command,
        canonical: spec.canonical.into(),
        score,
        threshold: spec.threshold,
    })
}

fn resolve_percent_candidate<F>(
    input: &str,
    tokens: &[String],
    spec: ResolverSpec,
    build_command: F,
) -> Option<ResolverCandidate>
where
    F: Fn(u8) -> CanonicalCommand,
{
    let percent = extract_percent_value(input)?;
    let score = score_spec(input, tokens, &spec)?;
    let command = build_command(percent);
    Some(ResolverCandidate {
        canonical: command.canonical_phrase(),
        command,
        score,
        threshold: spec.threshold,
    })
}

fn score_spec(input: &str, tokens: &[String], spec: &ResolverSpec) -> Option<f32> {
    if tokens.len() < spec.min_tokens {
        return None;
    }

    let required_scores: Vec<f32> = spec
        .required_groups
        .iter()
        .map(|group| best_group_score(tokens, group))
        .collect::<Option<Vec<_>>>()?;

    let required_avg = required_scores.iter().sum::<f32>() / required_scores.len() as f32;
    let alias_score = best_alias_score(input, tokens, spec.aliases);
    let optional_bonus = spec
        .optional_tokens
        .iter()
        .filter_map(|token| best_token_score(tokens, token))
        .filter(|score| *score >= 0.9)
        .count() as f32
        * 0.04;
    let negative_penalty = spec
        .negative_tokens
        .iter()
        .filter_map(|token| best_token_score(tokens, token))
        .filter(|score| *score >= 0.94)
        .count() as f32
        * 0.24;

    Some((required_avg * 0.86 + alias_score * 0.14 + optional_bonus - negative_penalty).clamp(0.0, 1.0))
}

fn hard_reject_reason(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some("empty transcript".into());
    }

    let tokens = tokenize(trimmed);
    if tokens.is_empty() {
        return Some("empty transcript".into());
    }

    if tokens.len() == 1 {
        return Some("single-token fragment".into());
    }

    if has_standalone_percent(trimmed)
        && !contains_any(
            trimmed,
            &[
                "govee",
                "rgb",
                "luefter",
                "fan",
                "lautstaerke",
                "volume",
                "bildschirm",
                "monitor",
                "helligkeit",
                "licht",
                "lichter",
                "lampe",
                "lampen",
            ],
        )
    {
        return Some("percentage without explicit target".into());
    }

    if tokens.len() <= 2
        && !contains_any(
            trimmed,
            &[
                "rgb",
                "govee",
                "licht",
                "lichter",
                "lampe",
                "luefter",
                "fan",
                "gpu",
                "tank",
                "oeffne",
                "starte",
                "screenshot",
                "bildschirm",
                "lautstaerke",
                "mute",
            ],
        )
    {
        return Some("short fragment without command anchor".into());
    }

    None
}

pub(crate) fn normalize_input(input: &str) -> String {
    let normalized = input
        .to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
        .replace(
            ['!', '?', ':', ';', ',', '.', '"', '\'', '(', ')', '%'],
            " ",
        );

    collapse_spaces(&normalized)
}

fn collapse_spaces(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn best_alias_score(input: &str, tokens: &[String], aliases: &[&str]) -> f32 {
    aliases
        .iter()
        .map(|alias| {
            let alias = normalize_input(alias);
            if input == alias {
                1.0
            } else if input.contains(&alias) {
                0.97
            } else {
                let alias_tokens = tokenize(&alias);
                if alias_tokens.is_empty() {
                    0.0
                } else {
                    alias_tokens
                        .iter()
                        .map(|token| best_token_score(tokens, token).unwrap_or(0.0))
                        .sum::<f32>()
                        / alias_tokens.len() as f32
                }
            }
        })
        .fold(0.0, f32::max)
}

fn best_group_score(tokens: &[String], group: &[&str]) -> Option<f32> {
    group.iter()
        .filter_map(|keyword| best_token_score(tokens, keyword))
        .max_by(|left, right| left.total_cmp(right))
}

fn best_token_score(tokens: &[String], keyword: &str) -> Option<f32> {
    let keyword = normalize_input(keyword);
    tokens
        .iter()
        .map(|token| approx_token_score(token, &keyword))
        .filter(|score| *score > 0.0)
        .max_by(|left, right| left.total_cmp(right))
}

fn approx_token_score(token: &str, keyword: &str) -> f32 {
    if token == keyword {
        return 1.0;
    }

    let token_len = token.chars().count();
    let keyword_len = keyword.chars().count();
    if token_len < 2 || keyword_len < 2 {
        return 0.0;
    }

    if token_len >= 5 && keyword_len >= 5 && (token.starts_with(keyword) || keyword.starts_with(token)) {
        return 0.93;
    }

    let distance = levenshtein(token, keyword);
    match distance {
        1 if token_len >= 4 && keyword_len >= 4 => 0.9,
        2 if token_len >= 6 && keyword_len >= 6 => 0.78,
        _ => 0.0,
    }
}

fn levenshtein(left: &str, right: &str) -> usize {
    let left_chars: Vec<char> = left.chars().collect();
    let right_chars: Vec<char> = right.chars().collect();
    let mut costs: Vec<usize> = (0..=right_chars.len()).collect();

    for (i, left_char) in left_chars.iter().enumerate() {
        let mut last_cost = i;
        costs[0] = i + 1;

        for (j, right_char) in right_chars.iter().enumerate() {
            let current_cost = costs[j + 1];
            let substitution_cost = usize::from(left_char != right_char);
            costs[j + 1] = std::cmp::min(
                std::cmp::min(costs[j + 1] + 1, costs[j] + 1),
                last_cost + substitution_cost,
            );
            last_cost = current_cost;
        }
    }

    costs[right_chars.len()]
}

fn extract_percent_value(input: &str) -> Option<u8> {
    let words: Vec<&str> = input.split_whitespace().collect();
    for word in words {
        let cleaned = word
            .trim_matches(|ch: char| !ch.is_ascii_digit())
            .trim_end_matches("prozent");
        let Ok(value) = cleaned.parse::<u16>() else {
            continue;
        };
        if (1..=100).contains(&value) {
            return u8::try_from(value).ok();
        }
    }
    None
}

fn has_standalone_percent(input: &str) -> bool {
    extract_percent_value(input).is_some() && !contains_any(input, &["auf", "helligkeit", "brightness", "lautstaerke"])
}

fn split_segments(input: &str) -> Vec<String> {
    input
        .replace(" und dann ", ",")
        .replace(" danach ", ",")
        .replace(" dann ", ",")
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn compose_reply(parts: &[String]) -> String {
    if parts.is_empty() {
        "Understood, Sir.".into()
    } else {
        parts.join(" ")
    }
}

fn contains_any(input: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| input.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::{
        hard_reject, resolve_closed_set_command, resolve_voice_command, CanonicalColor,
        CanonicalCommand,
    };

    #[test]
    fn resolves_rgb_power_off() {
        assert_eq!(
            resolve_closed_set_command("rgb aus"),
            Some(CanonicalCommand::RgbPower { on: false })
        );
    }

    #[test]
    fn resolves_all_lights_color() {
        assert_eq!(
            resolve_closed_set_command("alle lichter lila"),
            Some(CanonicalCommand::AllLightsColor(CanonicalColor::Lila))
        );
    }

    #[test]
    fn resolves_govee_brightness() {
        assert_eq!(
            resolve_closed_set_command("govee auf 50 prozent"),
            Some(CanonicalCommand::GoveeBrightness { percent: 50 })
        );
    }

    #[test]
    fn resolves_fan_speed() {
        assert_eq!(
            resolve_closed_set_command("luefter auf 80 prozent"),
            Some(CanonicalCommand::FanSpeed { percent: 80 })
        );
    }

    #[test]
    fn rejects_bare_percentage() {
        assert!(hard_reject("80 prozent"));
        assert_eq!(resolve_closed_set_command("80 prozent"), None);
    }

    #[test]
    fn rejects_broken_short_fragments() {
        assert!(hard_reject("wolfs"));
        assert!(hard_reject("laeu"));
        assert_eq!(resolve_closed_set_command("wolfs"), None);
        assert_eq!(resolve_closed_set_command("laeu"), None);
    }

    #[test]
    fn no_open_fallback_for_short_garbage() {
        let resolution = resolve_voice_command("wolfs");
        assert!(!resolution.accepted);
        assert!(!resolution.allow_open_fallback);
        assert!(resolution.reject_reason.is_some());
    }

    #[test]
    fn allows_open_fallback_for_longer_free_speech() {
        let resolution = resolve_voice_command("kannst du mir bitte erklaeren was heute anliegt");
        assert!(!resolution.accepted);
        assert!(resolution.allow_open_fallback);
    }
}
