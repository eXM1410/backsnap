#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VoicePhase {
    Idle,
    Entrance,
    Acknowledging,
    AwaitingCommand,
    Executing,
}

pub(crate) const VOICE_COMMAND_TIMEOUT_MS: u64 = 12_000;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TransitionEffects {
    pub(crate) stop_entrance: bool,
    pub(crate) stop_tts: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct VoiceState {
    pub(crate) phase: VoicePhase,
    deadline: Option<std::time::Instant>,
}

impl VoiceState {
    const fn idle() -> Self {
        Self {
            phase: VoicePhase::Idle,
            deadline: None,
        }
    }

    fn normalize(&mut self) {
        if let Some(deadline) = self.deadline {
            if std::time::Instant::now() >= deadline {
                self.phase = VoicePhase::Idle;
                self.deadline = None;
            }
        }
    }
}

static VOICE_STATE: std::sync::Mutex<VoiceState> = std::sync::Mutex::new(VoiceState::idle());

pub(crate) fn snapshot() -> VoiceState {
    if let Ok(mut state) = VOICE_STATE.lock() {
        state.normalize();
        *state
    } else {
        VoiceState::idle()
    }
}

pub(crate) fn set_phase(phase: VoicePhase, timeout_ms: Option<u64>, reason: &str) {
    if let Ok(mut state) = VOICE_STATE.lock() {
        state.phase = phase;
        state.deadline =
            timeout_ms.map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
        log::info!("[jarvis-voice] State -> {:?} ({reason})", phase);
    }
}

pub(crate) fn try_set_phase(
    allowed: &[VoicePhase],
    phase: VoicePhase,
    timeout_ms: Option<u64>,
    reason: &str,
) -> bool {
    if let Ok(mut state) = VOICE_STATE.lock() {
        state.normalize();
        if allowed.contains(&state.phase) {
            state.phase = phase;
            state.deadline = timeout_ms
                .map(|ms| std::time::Instant::now() + std::time::Duration::from_millis(ms));
            log::info!("[jarvis-voice] State -> {:?} ({reason})", phase);
            true
        } else {
            log::debug!(
                "[jarvis-voice] Ignoring state transition to {:?} from {:?} ({reason})",
                phase,
                state.phase
            );
            false
        }
    } else {
        false
    }
}

pub(crate) fn reset(reason: &str) {
    set_phase(VoicePhase::Idle, None, reason);
}

pub(crate) fn begin_clap_session() -> bool {
    try_set_phase(
        &[VoicePhase::Idle],
        VoicePhase::Entrance,
        None,
        "clap detected",
    )
}

pub(crate) fn arm_command_window_after_clap() {
    if snapshot().phase == VoicePhase::Entrance {
        set_phase(
            VoicePhase::AwaitingCommand,
            Some(VOICE_COMMAND_TIMEOUT_MS),
            "clap armed command window",
        );
    }
}

pub(crate) fn begin_wake_acknowledgement() -> Option<TransitionEffects> {
    let from_phase = snapshot().phase;
    if try_set_phase(
        &[VoicePhase::Idle, VoicePhase::Entrance],
        VoicePhase::Acknowledging,
        None,
        "wake acknowledgement",
    ) {
        Some(TransitionEffects {
            stop_entrance: from_phase == VoicePhase::Entrance,
            stop_tts: false,
        })
    } else {
        None
    }
}

pub(crate) fn finish_wake_acknowledgement() {
    if snapshot().phase == VoicePhase::Acknowledging {
        set_phase(
            VoicePhase::AwaitingCommand,
            Some(VOICE_COMMAND_TIMEOUT_MS),
            "wake acknowledged; awaiting command",
        );
    }
}

pub(crate) fn begin_voice_command() -> Option<TransitionEffects> {
    let from_phase = snapshot().phase;
    if try_set_phase(
        &[
            VoicePhase::Idle,
            VoicePhase::Entrance,
            VoicePhase::Acknowledging,
            VoicePhase::AwaitingCommand,
        ],
        VoicePhase::Executing,
        None,
        "voice command execution",
    ) {
        Some(TransitionEffects {
            stop_entrance: from_phase == VoicePhase::Entrance,
            stop_tts: from_phase == VoicePhase::Acknowledging,
        })
    } else {
        None
    }
}

pub(crate) fn finish_voice_command(reason: &str) {
    set_phase(VoicePhase::Idle, None, reason);
}
