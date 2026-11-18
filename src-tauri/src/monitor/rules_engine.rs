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
    pub shield_grace_until: Option<Instant>,
    pub bypass_grace_until: Option<Instant>,
}

impl Default for RulesEngine {
    fn default() -> Self {
        Self {
            pause_started_at: None,
            was_playing: false,
            custom_whitelist: HashSet::new(),
            custom_blacklist: HashSet::new(),
            shield_grace_until: None,
            bypass_grace_until: None,
        }
    }
}

impl RulesEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn grant_tactical_shield(&mut self, duration: Duration) {
        self.shield_grace_until = Some(Instant::now() + duration);
    }

    pub fn grant_bypass_grace(&mut self, duration: Duration) {
        self.bypass_grace_until = Some(Instant::now() + duration);
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

        // Bypass grace buffer suppresses repeated triggers immediately following a successful hold
        if let Some(until) = self.bypass_grace_until {
            if Instant::now() < until {
                return EvaluationResult::Allow {
                    reason: "Bypass grace period active",
                };
            } else {
                self.bypass_grace_until = None;
            }
        }

        // Tactical Shield grace buffer (5 minutes) grants temporary immunity
        if let Some(until) = self.shield_grace_until {
            if Instant::now() < until {
                return EvaluationResult::Allow {
                    reason: "Tactical Shield 5-minute grace buffer active",
                };
            } else {
                self.shield_grace_until = None;
            }
        }

        let proc = window.process_name.to_lowercase();

        // 1. User custom whitelist overrides all defaults
        if self.custom_whitelist.contains(&proc) {
            if media.is_playing {
                self.was_playing = true;
                self.pause_started_at = None;
            }
            return EvaluationResult::Allow {
                reason: "User custom whitelisted application",
            };
        }

        // 2. Blacklisted apps trigger immediate barrier
        if self.custom_blacklist.contains(&proc) || DEFAULT_BLACKLIST.contains(&proc.as_str()) {
            return EvaluationResult::TriggerBarrier {
                reason: "Blacklisted distraction application active",
            };
        }

        // 3. Process lookup failure check: fail closed if title matches blacklisted keywords
        if window.lookup_failed && !window.title.is_empty() {
            let t = window.title.to_lowercase();
            let words: std::collections::HashSet<&str> = t
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect();

            let blacklist_items = DEFAULT_BLACKLIST
                .iter()
                .copied()
                .chain(self.custom_blacklist.iter().map(|s| s.as_str()));

            for item in blacklist_items {
                let stem = item.strip_suffix(".exe").unwrap_or(item);
                if words.contains(stem) {
                    return EvaluationResult::TriggerBarrier {
                        reason: "Blocked application matching distraction signature",
                    };
                }
            }
        }

        // 4. Whitelisted development tools are always allowed
        if DEFAULT_WHITELIST.contains(&proc.as_str()) {
            if media.is_playing {
                self.was_playing = true;
                self.pause_started_at = None;
            }
            return EvaluationResult::Allow {
                reason: "Active development tool whitelisted",
            };
        }

        // 5. Task Buddy web app is allowed in web browsers
        let is_browser = matches!(
            proc.as_str(),
            "chrome.exe" | "msedge.exe" | "firefox.exe" | "brave.exe" | "opera.exe"
        );
        if is_browser {
            let title_lower = window.title.to_lowercase();
            if title_lower.ends_with(" - task buddy")
                || title_lower.starts_with("task buddy - ")
                || title_lower.contains("localhost:3000")
                || title_lower.contains("taskbuddy.app")
            {
                if media.is_playing {
                    self.was_playing = true;
                    self.pause_started_at = None;
                }
                return EvaluationResult::Allow {
                    reason: "Task Buddy web application active",
                };
            }
        }

        // 6. Media playback tracking
        if media.is_playing {
            self.was_playing = true;
            self.pause_started_at = None;

            // Missing metadata or verified unaligned media triggers barrier
            if media.metadata_missing {
                return EvaluationResult::TriggerBarrier {
                    reason: "Media playing without identifiable instructional metadata",
                };
            }

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
            lookup_failed: false,
        }
    }

    fn dummy_media(playing: bool, title: &str) -> MediaStatus {
        MediaStatus {
            has_active_media: true,
            is_playing: playing,
            title: title.to_string(),
            artist: "".to_string(),
            source_app_id: "".to_string(),
            metadata_missing: false,
        }
    }

    #[test]
    fn test_free_time_allows_everything() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("steam.exe");
        let med = dummy_media(true, "Funny cats");
        let res = engine.evaluate(false, &win, &med, false);
        assert_eq!(res, EvaluationResult::Allow { reason: "Free Time active (0 barriers)" });
    }

    #[test]
    fn test_default_blacklist_triggers_barrier() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("steam.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Blacklisted distraction application active" });
    }

    #[test]
    fn test_custom_blacklist_triggers_barrier() {
        let mut engine = RulesEngine::new();
        engine.custom_blacklist.insert("customgame.exe".to_string());
        let win = dummy_window("customgame.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Blacklisted distraction application active" });
    }

    #[test]
    fn test_tactical_shield_allows_temporary_access() {
        let mut engine = RulesEngine::new();
        engine.grant_tactical_shield(Duration::from_secs(300));
        let win = dummy_window("steam.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::Allow { reason: "Tactical Shield 5-minute grace buffer active" });
    }

    #[test]
    fn test_task_buddy_browser_window_allowed() {
        let mut engine = RulesEngine::new();
        let mut win = dummy_window("chrome.exe");
        win.title = "Task Buddy - Pomodoro Focus".to_string();
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::Allow { reason: "Task Buddy web application active" });
    }

    #[test]
    fn test_media_misaligned_triggers_barrier() {
        let mut engine = RulesEngine::new();
        let win = dummy_window("chrome.exe");
        let med = dummy_media(true, "Top 10 funny cat videos");
        let res = engine.evaluate(true, &win, &med, false);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Media playback does not align with declared hourly task" });
    }

    #[test]
    fn test_pause_grace_period_expiry() {
        let mut engine = RulesEngine::new();
        engine.was_playing = true;
        engine.pause_started_at = Some(Instant::now() - Duration::from_secs(35));
        let win = dummy_window("chrome.exe");
        let mut med = dummy_media(false, "Coding music");
        med.has_active_media = true;
        let res = engine.evaluate(true, &win, &med, true);
        assert_eq!(res, EvaluationResult::TriggerBarrier { reason: "Tutorial paused beyond 30-second coding grace buffer" });
    }

    #[test]
    fn test_process_lookup_failed_word_boundary_matching() {
        let mut engine = RulesEngine::new();
        let mut win = dummy_window("");
        win.lookup_failed = true;
        win.title = "Upstreaming commit to repo".to_string();
        let med = dummy_media(false, "");
        let res1 = engine.evaluate(true, &win, &med, true);
        assert_ne!(res1, EvaluationResult::TriggerBarrier { reason: "Blocked application matching distraction signature" });

        win.title = "Steam Store - Special Offers".to_string();
        let res2 = engine.evaluate(true, &win, &med, true);
        assert_eq!(res2, EvaluationResult::TriggerBarrier { reason: "Blocked application matching distraction signature" });
    }

    #[test]
    fn test_custom_whitelist_overrides_default_blacklist() {
        let mut engine = RulesEngine::new();
        engine.custom_whitelist.insert("discord.exe".to_string());

        let win = dummy_window("discord.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);

        assert_eq!(res, EvaluationResult::Allow { reason: "User custom whitelisted application" });
    }

    #[test]
    fn test_bypass_grace_period() {
        let mut engine = RulesEngine::new();
        engine.grant_bypass_grace(Duration::from_secs(10));

        let win = dummy_window("steam.exe");
        let med = dummy_media(false, "");
        let res = engine.evaluate(true, &win, &med, true);

        assert_eq!(res, EvaluationResult::Allow { reason: "Bypass grace period active" });
    }

    #[test]
    fn test_whitelisted_ide_clears_pause_when_media_playing() {
        let mut engine = RulesEngine::new();
        engine.was_playing = true;
        engine.pause_started_at = Some(Instant::now());

        let win = dummy_window("code.exe");
        let med = dummy_media(true, "Rust tutorial");
        let res = engine.evaluate(true, &win, &med, true);

        assert_eq!(res, EvaluationResult::Allow { reason: "Active development tool whitelisted" });
        assert!(engine.pause_started_at.is_none());
    }
}
