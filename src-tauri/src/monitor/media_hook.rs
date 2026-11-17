use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MediaStatus {
    pub has_active_media: bool,
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub source_app_id: String,
}

impl Default for MediaStatus {
    fn default() -> Self {
        Self {
            has_active_media: false,
            is_playing: false,
            title: String::new(),
            artist: String::new(),
            source_app_id: String::new(),
        }
    }
}

pub fn get_current_media_status() -> MediaStatus {
    let Ok(manager_async) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() else {
        return MediaStatus::default();
    };

    let Ok(manager) = manager_async.join() else {
        return MediaStatus::default();
    };

    let Ok(current_session): windows::core::Result<GlobalSystemMediaTransportControlsSession> =
        manager.GetCurrentSession()
    else {
        return MediaStatus::default();
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

    if let Ok(props_async) = current_session.TryGetMediaPropertiesAsync() {
        if let Ok(props) = props_async.join() {
            title = props.Title().map(|s| s.to_string()).unwrap_or_default();
            artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        }
    }

    MediaStatus {
        has_active_media: true,
        is_playing,
        title,
        artist,
        source_app_id,
    }
}
