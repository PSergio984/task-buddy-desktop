pub mod window_hook;
pub mod media_hook;
pub mod rules_engine;

pub use window_hook::{get_active_window_info, ActiveWindowInfo};
pub use media_hook::{get_current_media_status, MediaStatus};
pub use rules_engine::{EvaluationResult, RulesEngine};
