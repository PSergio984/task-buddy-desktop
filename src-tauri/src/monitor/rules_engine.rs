use std::collections::HashSet;
use std::time::{Duration, Instant};
use super::media_hook::MediaStatus;
use super::window_hook::ActiveWindowInfo;

const CODING_GRACE_BUFFER_SECS: u64 = 30;

pub const DEFAULT_WHITELIST: &[&str] = &[
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

pub const DEFAULT_BLACKLIST: &[&str] = &[
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
    pub custom_whitelist: HashSet<String>,
    pub custom_blacklist: HashSet<String>,
}

impl Default for RulesEngine {
    fn default() -> Self {
        Self {
            pause_started_at: None,
            was_playing: false,
            custom_whitelist: HashSet::new(),
            custom_blacklist: HashSet::new(),
        }
    }
}

impl RulesEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_rules(&mut self, rules: &[crate::db::ProcessRule]) {
        self.custom_whitelist.clear();
        self.custom_blacklist.clear();

        for r in rules {
            let name = r.process_name.to_lowercase();
            match r.category.as_str() {
                "whitelist" => {
                    self.custom_whitelist.insert(name);
                }
                "blacklist" => {
                    self.custom_blacklist.insert(name);
                }
                _ => {}
            }
        }
    }

    pub fn evaluate(
        &mut self,
        is_focus_active: bool,
        window: &ActiveWindowInfo,
        media: &MediaStatus,
        is_media_aligned: bool,
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
        if DEFAULT_BLACKLIST.contains(&proc.as_str()) || self.custom_blacklist.contains(&proc) {
            return EvaluationResult::TriggerBarrier {
                reason: "Blacklisted distraction application active",
            };
        }

        // 2. Whitelisted development tools are always allowed
        if DEFAULT_WHITELIST.contains(&proc.as_str()) || self.custom_whitelist.contains(&proc) {
            // User is actively in IDE or terminal, keep them in the zone
            return EvaluationResult::Allow {
                reason: "Active development tool whitelisted",
            };
        }

        // 2.5 Task Buddy web app is always allowed in any browser
        let title_lower = window.title.to_lowercase();
        if title_lower.contains("task buddy") || title_lower.contains("localhost:3000") {
            return EvaluationResult::Allow {
                reason: "Task Buddy web application whitelisted",
            };
        }

        // 3. Media playback tracking
        if media.is_playing {
            self.was_playing = true;
            self.pause_started_at = None;

            // If media is verified unaligned with declared task, trigger barrier
            if !is_media_aligned && !media.title.is_empty() {
                return EvaluationResult::TriggerBarrier {
                    reason: "Media playback does not align with declared hourly task",
                };
            }

            return EvaluationResult::Allow {
                reason: "Tutorial media is actively playing and aligned",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_window(proc: &str) -> ActiveWindowInfo {
        ActiveWindowInfo {
            title: "Test Window".to_string(),
            process_name: proc.to_string(),
            is_empty: false,
        }
    }

    fn dummy_media(playing: bool, title: &str) -> MediaStatus {
        MediaStatus {
            has_active_media: true,
            is_playing: playing,
            title: title.to_string(),
            artist: "Test Artist".to_string(),
            source_app_id: "chrome.exe".to_string(),
        }
    }

    #[test]
    fn test_free_time_always_allows() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("steam.exe");
        let med = dummy_media(false, "Random");
        let res = engine.evaluate(false, &win, &med, true);
        assert_eq!(res, EvaluationResult::Allow { reason: "Free Time active (0 barriers)" });
    }

    #[test]
    fn test_blacklisted_app_triggers_barrier() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("discord.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Blacklisted distraction application active" });
    }

    #[test]
    fn test_whitelisted_ide_always_allows() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("cursor.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::Allow { reason: "Active development tool whitelisted" });
    }

    #[test]
    fn test_media_misaligned_triggers_barrier() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("chrome.exe");
        let med = dummy_media(true, "Top Funny Cat Clips");
        let res = engine.evaluate(true, &win, &med, false);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Media playback does not align with declared hourly task" });
    }

    #[test]
    fn test_media_aligned_playing_allows() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("chrome.exe");
        let med = dummy_media(true, "Rust Async in Tauri");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::Allow { reason: "Tutorial media is actively playing and aligned" });
    }

    #[test]
    fn test_coding_grace_buffer_trigger() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("chrome.exe");
        
        // 1. Media playing
        let med_playing = dummy_media(true, "Rust Tutorial");
        engine.evaluate(true, &win, &med_playing, true);
        assert!(engine.was_playing);

        // 2. Media paused just now (within 30s)
        let med_paused = dummy_media(false, "Rust Tutorial");
        let res1 = engine.evaluate(true, &win, &med_paused, true);
        assert_eq!(res1, EvaluationResult::Allow { reason: "Within 30-second coding grace buffer" });

        // 3. Fast-forward pause time past 30 seconds
        engine.pause_started_at = Some(Instant::now() - Duration::from_secs(35));
        let res2 = engine.evaluate(true, &win, &med_paused, true);
        assert_eq!(res2, EvaluationResult::TriggerBarrier { reason: "Tutorial paused beyond 30-second coding grace buffer" });
    }

    #[test]
    fn test_task_buddy_browser_window_is_whitelisted() {
        let mut engine = RulesEngine::new();
        let mut win = dummy_window("chrome.exe");
        win.title = "Task Buddy — Focus & Planner".to_string();
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(
            res,
            EvaluationResult::Allow {
                reason: "Task Buddy web application whitelisted"
            }
        );
    }
}

