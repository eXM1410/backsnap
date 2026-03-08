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

fn parse_segment(segment: &str, inherited_scope: Option<Scope>) -> Option<ParsedSegment> {
    if let Some(parsed) = parse_query(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_watering(segment) {
        return Some(parsed);
    }

    if let Some(parsed) = parse_fan(segment) {
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
        let scope = scope.unwrap_or(Scope::All);
        actions.push(brightness_action(scope, brightness));
        reply_parts.push(brightness_reply(scope, brightness));
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
            "tankinhalt",
            "wie voll ist der tank",
            "wie warm ist das zelt",
            "zelt temperatur",
            "zelt luftfeuchtigkeit",
        ],
    ) {
        return Some(ParsedSegment {
            scope: None,
            reply: "Checking the tent.".into(),
            actions: vec![KeywordAction {
                action: "tent_status".into(),
                params: serde_json::json!({}),
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

fn normalize_input(input: &str) -> String {
    input
        .to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
        .replace(['!', '?', ':', ';'], ",")
        .replace("  ", " ")
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
