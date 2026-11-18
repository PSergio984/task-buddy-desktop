use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct MediaStatus {
    pub has_active_media: bool,
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub source_app_id: String,
    pub metadata_missing: bool,
}

use std::sync::atomic::{AtomicBool, Ordering};

static LOGGED_REQUEST_ASYNC_ERR: AtomicBool = AtomicBool::new(false);
static LOGGED_JOIN_ERR: AtomicBool = AtomicBool::new(false);

pub fn get_current_media_status() -> MediaStatus {
    // Ensure WinRT apartment is initialized on the current thread
    unsafe {
        let _ = windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        );
    }

    let manager_async = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync() {
        Ok(m) => {
            LOGGED_REQUEST_ASYNC_ERR.store(false, Ordering::Relaxed);
            m
        }
        Err(e) => {
            if !LOGGED_REQUEST_ASYNC_ERR.swap(true, Ordering::Relaxed) {
                eprintln!("[media_hook] WinRT RequestAsync failed (logged once): {e:?}");
            }
            return MediaStatus::default();
        }
    };

    let manager = match manager_async.join() {
        Ok(m) => {
            LOGGED_JOIN_ERR.store(false, Ordering::Relaxed);
            m
        }
        Err(e) => {
            if !LOGGED_JOIN_ERR.swap(true, Ordering::Relaxed) {
                eprintln!("[media_hook] SessionManager join failed (logged once): {e:?}");
            }
            return MediaStatus::default();
        }
    };

    let current_session: GlobalSystemMediaTransportControlsSession = match manager.GetCurrentSession() {
        Ok(s) => s,
        Err(_) => return MediaStatus::default(),
    };

    let source_app_id = current_session
        .SourceAppUserModelId()
        .map(|s| s.to_string())
        .unwrap_or_default();

    let mut is_playing = false;
    if let Ok(playback_info) = current_session.GetPlaybackInfo() {
        if let Ok(status) = playback_info.PlaybackStatus() {
            is_playing = status == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        }
    }

    let mut title = String::new();
    let mut artist = String::new();
    let mut metadata_missing = false;

    if let Ok(props_async) = current_session.TryGetMediaPropertiesAsync() {
        if let Ok(props) = props_async.join() {
            title = props.Title().map(|s| s.to_string()).unwrap_or_default();
            artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        } else {
            metadata_missing = is_playing;
        }
    } else {
        metadata_missing = is_playing;
    }

    if is_playing && title.trim().is_empty() {
        metadata_missing = true;
    }

    MediaStatus {
        has_active_media: true,
        is_playing,
        title,
        artist,
        source_app_id,
        metadata_missing,
    }
}
