pub mod ai;
pub mod db;
pub mod monitor;
pub mod sync;

use chrono::Utc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    menu::{MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub remote_session_id: Option<i64>,
    pub is_focus_active: bool,
    pub task_title: String,
    pub duration_minutes: u32,
    pub elapsed_seconds: u32,
    pub is_overlay_visible: bool,
    pub backend_token: Option<String>,
}

pub struct AppState {
    pub session: Arc<Mutex<SessionState>>,
    pub db: db::Database,
    pub sync_client: sync::BackendSyncClient,
    pub matcher: ai::ContentMatcher,
}

#[tauri::command]
fn get_session_status(state: State<'_, AppState>) -> SessionState {
    state.session.lock().unwrap().clone()
}

#[tauri::command]
fn get_live_os_info() -> (monitor::ActiveWindowInfo, monitor::MediaStatus) {
    (
        monitor::get_active_window_info(),
        monitor::get_current_media_status(),
    )
}

#[tauri::command]
fn get_local_rules(state: State<'_, AppState>) -> Result<Vec<db::ProcessRule>, String> {
    state.db.get_rules().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_local_rule(
    state: State<'_, AppState>,
    process_name: String,
    category: String,
) -> Result<(), String> {
    state
        .db
        .add_rule(&process_name, &category)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_local_rule(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    state.db.delete_rule(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_recent_sessions(
    state: State<'_, AppState>,
    limit: u32,
) -> Result<Vec<db::FocusSessionRecord>, String> {
    state.db.get_recent_sessions(limit).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_groq_api_key(state: State<'_, AppState>, key: Option<String>) -> Result<(), String> {
    state.matcher.set_api_key(key);
    Ok(())
}

#[tauri::command]
async fn check_content_alignment(
    state: State<'_, AppState>,
    task_title: String,
    media_title: String,
) -> Result<ai::MatchResult, String> {
    Ok(state.matcher.is_aligned(&task_title, &media_title).await)
}

#[tauri::command]
async fn start_focus_session(
    app: AppHandle,
    state: State<'_, AppState>,
    task_title: String,
    duration_minutes: u32,
    token: Option<String>,
) -> Result<SessionState, String> {
    let session_uuid = format!("sess_{}", Utc::now().timestamp_millis());
    let clean_title = if task_title.trim().is_empty() {
        "Deep Work".to_string()
    } else {
        task_title.trim().to_string()
    };

    // Attempt remote backend session creation if token is provided
    let mut remote_id = None;
    if let Some(ref auth_token) = token {
        if !auth_token.is_empty() {
            if let Ok(id) = state
                .sync_client
                .start_remote_session(auth_token, duration_minutes, &clean_title)
                .await
            {
                remote_id = Some(id);
            }
        }
    }

    // Record in local SQLite
    let record = db::FocusSessionRecord {
        id: session_uuid.clone(),
        task_id: None,
        task_title: clean_title.clone(),
        started_at: Utc::now().to_rfc3339(),
        ended_at: None,
        target_duration_minutes: duration_minutes,
        actual_duration_seconds: 0,
        status: "running".to_string(),
        diversion_count: 0,
        synced_to_backend: remote_id.is_some(),
    };
    let _ = state.db.insert_session(&record);

    let mut session = state.session.lock().unwrap();
    session.session_id = session_uuid;
    session.remote_session_id = remote_id;
    session.is_focus_active = true;
    session.task_title = clean_title;
    session.duration_minutes = duration_minutes;
    session.elapsed_seconds = 0;
    session.is_overlay_visible = false;
    session.backend_token = token;

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
async fn end_focus_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionState, String> {
    let mut session = state.session.lock().unwrap();
    session.is_focus_active = false;
    session.is_overlay_visible = false;

    let sess_id = session.session_id.clone();
    let remote_id = session.remote_session_id;
    let token = session.backend_token.clone();
    let elapsed = session.elapsed_seconds;

    // Update local SQLite record
    let _ = state.db.update_session_end(
        &sess_id,
        &Utc::now().to_rfc3339(),
        elapsed,
        "completed",
        0,
    );

    // Stop remote session if connected
    if let (Some(t), Some(rid)) = (token, remote_id) {
        let client = state.sync_client.clone();
        tauri::async_runtime::spawn(async move {
            let _ = client.stop_remote_session(&t, rid).await;
        });
    }

    // Ensure overlay is hidden if active
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn trigger_barrier(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    session.is_overlay_visible = true;

    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.show();
        let _ = overlay.set_focus();
    }

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[tauri::command]
fn dismiss_barrier(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    session.is_overlay_visible = false;

    let sess_id = session.session_id.clone();

    // Log bypass in SQLite
    let div_record = db::DiversionEventRecord {
        id: format!("div_{}", Utc::now().timestamp_millis()),
        session_id: sess_id,
        timestamp: Utc::now().to_rfc3339(),
        trigger_type: "manual_bypass".to_string(),
        detected_title: None,
        bypassed: true,
    };
    let _ = state.db.record_diversion(&div_record);

    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_instance = db::Database::new("task_buddy_desktop.db").expect("Failed to initialize SQLite database");
    let sync_client = sync::BackendSyncClient::default();
    let matcher = ai::ContentMatcher::new();

    let app_state = AppState {
        session: Arc::new(Mutex::new(SessionState::default())),
        db: db_instance.clone(),
        sync_client,
        matcher: matcher.clone(),
    };

    let session_ref = app_state.session.clone();
    let db_ref = db_instance.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_session_status,
            get_live_os_info,
            get_local_rules,
            add_local_rule,
            delete_local_rule,
            get_recent_sessions,
            set_groq_api_key,
            check_content_alignment,
            start_focus_session,
            end_focus_session,
            trigger_barrier,
            dismiss_barrier,
        ])
        .setup(move |app| {
            // Spawn Background OS Monitor Thread with dynamic SQLite rules and AI matcher
            let monitor_handle = app.handle().clone();
            let monitor_session = session_ref.clone();
            let monitor_db = db_ref.clone();
            let monitor_matcher = matcher.clone();

            std::thread::spawn(move || {
                let mut rules = monitor::RulesEngine::new();
                let mut last_rules_update = std::time::Instant::now();

                // Initial rules load
                if let Ok(initial_rules) = monitor_db.get_rules() {
                    rules.update_rules(&initial_rules);
                }

                loop {
                    std::thread::sleep(Duration::from_millis(500));

                    // Refresh custom rules from SQLite every 5 seconds
                    if last_rules_update.elapsed() > Duration::from_secs(5) {
                        if let Ok(updated_rules) = monitor_db.get_rules() {
                            rules.update_rules(&updated_rules);
                        }
                        last_rules_update = std::time::Instant::now();
                    }

                    let (is_focus_active, active_task_title) = {
                        let lock = monitor_session.lock().unwrap();
                        (lock.is_focus_active, lock.task_title.clone())
                    };

                    let window = monitor::get_active_window_info();
                    let media = monitor::get_current_media_status();

                    // Evaluate content alignment if media is active
                    let is_media_aligned = if media.has_active_media && !media.title.is_empty() && is_focus_active {
                        monitor_matcher.evaluate_offline(&active_task_title, &media.title).aligned
                    } else {
                        true
                    };

                    let eval = rules.evaluate(is_focus_active, &window, &media, is_media_aligned);
                    if let monitor::EvaluationResult::TriggerBarrier { reason } = eval {
                        let mut session = monitor_session.lock().unwrap();
                        if session.is_focus_active && !session.is_overlay_visible {
                            session.is_overlay_visible = true;
                            let snapshot = session.clone();
                            let sess_id = session.session_id.clone();
                            drop(session);

                            // Record diversion event in SQLite
                            let div = db::DiversionEventRecord {
                                id: format!("div_{}", Utc::now().timestamp_millis()),
                                session_id: sess_id,
                                timestamp: Utc::now().to_rfc3339(),
                                trigger_type: reason.to_string(),
                                detected_title: Some(window.title.clone()),
                                bypassed: false,
                            };
                            let _ = monitor_db.record_diversion(&div);

                            if let Some(overlay) = monitor_handle.get_webview_window("overlay") {
                                let _ = overlay.show();
                                let _ = overlay.set_focus();
                            }
                            let _ = monitor_handle.emit("session-changed", &snapshot);
                            let _ = monitor_handle.emit("barrier-triggered", reason);
                        }
                    }
                }
            });

            // Build Tray Menu Items
            let header_item = MenuItem::with_id(
                app,
                "header",
                "🟢 Task Buddy • Free Time",
                false,
                None::<&str>,
            )?;
            let start_25 = MenuItem::with_id(
                app,
                "start_25",
                "▶ Start 25m Focus Block",
                true,
                None::<&str>,
            )?;
            let start_50 = MenuItem::with_id(
                app,
                "start_50",
                "▶ Start 50m Focus Block",
                true,
                None::<&str>,
            )?;
            let end_session = MenuItem::with_id(
                app,
                "end_session",
                "⏹ End Session (Free Time)",
                true,
                None::<&str>,
            )?;
            let toggle_overlay = MenuItem::with_id(
                app,
                "toggle_overlay",
                "⚡ Test 6s Friction Barrier",
                true,
                None::<&str>,
            )?;
            let show_main =
                MenuItem::with_id(app, "show_main", "📱 Open Dashboard", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "❌ Quit", true, None::<&str>)?;

            let menu = MenuBuilder::new(app)
                .item(&header_item)
                .item(&PredefinedMenuItem::separator(app)?)
                .item(&start_25)
                .item(&start_50)
                .item(&end_session)
                .item(&PredefinedMenuItem::separator(app)?)
                .item(&toggle_overlay)
                .item(&show_main)
                .item(&PredefinedMenuItem::separator(app)?)
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "start_25" => {
                        let state = app.state::<AppState>();
                        let mut session = state.session.lock().unwrap();
                        session.session_id = format!("sess_{}", Utc::now().timestamp_millis());
                        session.is_focus_active = true;
                        session.duration_minutes = 25;
                        session.elapsed_seconds = 0;
                        session.task_title = "Focus Session (25m)".to_string();
                        let snapshot = session.clone();
                        drop(session);
                        let _ = app.emit("session-changed", &snapshot);
                    }
                    "start_50" => {
                        let state = app.state::<AppState>();
                        let mut session = state.session.lock().unwrap();
                        session.session_id = format!("sess_{}", Utc::now().timestamp_millis());
                        session.is_focus_active = true;
                        session.duration_minutes = 50;
                        session.elapsed_seconds = 0;
                        session.task_title = "Deep Focus (50m)".to_string();
                        let snapshot = session.clone();
                        drop(session);
                        let _ = app.emit("session-changed", &snapshot);
                    }
                    "end_session" => {
                        let state = app.state::<AppState>();
                        let mut session = state.session.lock().unwrap();
                        session.is_focus_active = false;
                        session.is_overlay_visible = false;
                        let snapshot = session.clone();
                        drop(session);
                        if let Some(overlay) = app.get_webview_window("overlay") {
                            let _ = overlay.hide();
                        }
                        let _ = app.emit("session-changed", &snapshot);
                    }
                    "toggle_overlay" => {
                        if let Some(overlay) = app.get_webview_window("overlay") {
                            if overlay.is_visible().unwrap_or(false) {
                                let _ = overlay.hide();
                            } else {
                                let _ = overlay.show();
                                let _ = overlay.set_focus();
                            }
                        }
                    }
                    "show_main" => {
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                            let _ = main.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
