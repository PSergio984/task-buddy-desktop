use std::time::{Duration, Instant};
use super::media_hook::MediaStatus;
use super::window_hook::ActiveWindowInfo;

const CODING_GRACE_BUFFER_SECS: u64 = 30;

pub const WHITELISTED_DEV_APPS: &[&str] = &[
    "code.exe",
    "cursor.exe",
    "windowsterminal.exe",
    "powershell.exe",
    "cmd.exe",
    "devenv.exe",
    "task-buddy-desktop.exe",
    "idea64.exe",
    "pycharm64.exe",
];

pub const BLACKLISTED_APPS: &[&str] = &[
    "steam.exe",
    "discord.exe",
    "spotify.exe",
    "telegram.exe",
    "epicgameslauncher.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationResult {
    Allow { reason: &'static str },
    TriggerBarrier { reason: &'static str },
}

pub struct RulesEngine {
    pub pause_started_at: Option<Instant>,
    pub was_playing: bool,
}

impl Default for RulesEngine {
    fn default() -> Self {
        Self {
            pause_started_at: None,
            was_playing: false,
        }
    }
}

impl RulesEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate(
        &mut self,
        is_focus_active: bool,
        window: &ActiveWindowInfo,
        media: &MediaStatus,
    ) -> EvaluationResult {
        // In Free Time, barriers are 100% disabled
        if !is_focus_active {
            self.pause_started_at = None;
            self.was_playing = media.is_playing;
            return EvaluationResult::Allow {
                reason: "Free Time active (0 barriers)",
            };
        }

        let proc = window.process_name.to_lowercase();

        // 1. Blacklisted apps trigger immediate barrier
        if BLACKLISTED_APPS.contains(&proc.as_str()) {
            return EvaluationResult::TriggerBarrier {
                reason: "Blacklisted distraction application active",
            };
        }

        // 2. Whitelisted development tools are always allowed
        if WHITELISTED_DEV_APPS.contains(&proc.as_str()) {
            // User is actively in IDE or terminal, keep them in the zone
            return EvaluationResult::Allow {
                reason: "Active development tool whitelisted",
            };
        }

        // 3. Media playback tracking
        if media.is_playing {
            self.was_playing = true;
            self.pause_started_at = None;
            return EvaluationResult::Allow {
                reason: "Tutorial media is actively playing",
            };
        }

        // Media is paused or stopped
        if self.was_playing && !media.is_playing {
            let pause_time = self.pause_started_at.get_or_insert_with(Instant::now);
            let elapsed = pause_time.elapsed();

            if elapsed > Duration::from_secs(CODING_GRACE_BUFFER_SECS) {
                return EvaluationResult::TriggerBarrier {
                    reason: "Tutorial paused beyond 30-second coding grace buffer",
                };
            }

            return EvaluationResult::Allow {
                reason: "Within 30-second coding grace buffer",
            };
        }

        EvaluationResult::Allow {
            reason: "On-task activity",
        }
    }
}
