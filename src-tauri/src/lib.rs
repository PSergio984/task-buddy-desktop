#![allow(clippy::collapsible_if)]

pub mod ai;
pub mod db;
pub mod monitor;
pub mod sync;

use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{
    menu::{MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

fn show_all_overlays(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.set_always_on_top(true);
        let _ = overlay.show();
        let _ = overlay.set_focus();
    }
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay_") {
            let _ = window.set_always_on_top(true);
            let _ = window.show();
        }
    }
}

fn hide_all_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay") {
            let _ = window.hide();
        }
    }
}

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub remote_session_id: Option<i64>,
    pub is_focus_active: bool,
    pub task_title: String,
    pub duration_minutes: u32,
    pub elapsed_seconds: u32,
    pub is_overlay_visible: bool,
    #[serde(skip_serializing)]
    pub backend_token: Option<String>,
    pub shield_spent: bool,
    pub tactical_shield_until: Option<i64>,
    pub diversion_count: u32,
    #[serde(skip)]
    pub started_at_instant: Option<std::time::Instant>,
}

pub struct AppState {
    pub session: Arc<Mutex<SessionState>>,
    pub db: db::Database,
    pub sync_client: sync::BackendSyncClient,
    pub matcher: ai::ContentMatcher,
    pub session_generation: Arc<AtomicU64>,
    pub rules: Arc<Mutex<monitor::RulesEngine>>,
}

#[tauri::command]
fn get_session_status(state: State<'_, AppState>) -> SessionState {
    let session = state.session.lock().unwrap();
    let current_elapsed = if session.is_focus_active {
        session.elapsed_seconds
            + session
                .started_at_instant
                .map(|i| i.elapsed().as_secs() as u32)
                .unwrap_or(0)
    } else {
        session.elapsed_seconds
    };
    let mut snapshot = session.clone();
    snapshot.elapsed_seconds = current_elapsed;
    snapshot
}

#[tauri::command]
async fn get_live_os_info() -> Result<(monitor::ActiveWindowInfo, monitor::MediaStatus), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let win = monitor::get_active_window_info();
        let med = monitor::get_current_media_status();
        (win, med)
    })
    .await
    .map_err(|e| e.to_string())
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

fn internal_start_session(
    app: &AppHandle,
    state: &AppState,
    task_title: String,
    duration_minutes: u32,
    remote_id: Option<i64>,
    token: Option<String>,
) -> Result<SessionState, String> {
    let mut session = state.session.lock().unwrap();

    // If a session was already active, finalize its previous SQLite record
    if session.is_focus_active && !session.session_id.is_empty() {
        let ended_at = Utc::now().to_rfc3339();
        let actual_sec = session.elapsed_seconds
            + session
                .started_at_instant
                .map(|i| i.elapsed().as_secs() as u32)
                .unwrap_or(0);
        let _ = state.db.update_session_end(
            &session.session_id,
            &ended_at,
            actual_sec,
            "aborted",
            session.diversion_count,
        );
        if let (Some(tok), Some(rid)) = (&session.backend_token, session.remote_session_id) {
            let client = state.sync_client.clone();
            let tok_clone = tok.clone();
            tauri::async_runtime::spawn(async move {
                let _ = client.stop_remote_session(&tok_clone, rid).await;
            });
        }
    }

    let session_uuid = format!("sess_{}", Utc::now().timestamp_millis());
    let clean_title = if task_title.trim().is_empty() {
        "Deep Work".to_string()
    } else {
        task_title.trim().to_string()
    };

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
    if let Err(e) = state.db.insert_session(&record) {
        if let (Some(tok), Some(rid)) = (token.clone(), remote_id) {
            let client = state.sync_client.clone();
            tauri::async_runtime::spawn(async move {
                let _ = client.stop_remote_session(&tok, rid).await;
            });
        }
        return Err(format!("Failed to persist focus session to database: {e}"));
    }

    session.session_id = session_uuid;
    session.remote_session_id = remote_id;
    session.is_focus_active = true;
    session.task_title = clean_title;
    session.duration_minutes = duration_minutes;
    session.elapsed_seconds = 0;
    session.is_overlay_visible = false;
    session.backend_token = token;
    session.shield_spent = false;
    session.tactical_shield_until = None;
    session.diversion_count = 0;
    session.started_at_instant = Some(std::time::Instant::now());

    let snapshot = session.clone();
    drop(session);

    let _ = app.emit("session-changed", &snapshot);
    Ok(snapshot)
}

fn internal_end_session(app: &AppHandle, state: &AppState) -> SessionState {
    state.session_generation.fetch_add(1, Ordering::SeqCst);
    let mut session = state.session.lock().unwrap();
    let was_active = session.is_focus_active;
    session.is_focus_active = false;
    session.is_overlay_visible = false;

    let sess_id = session.session_id.clone();
    let remote_id = session.remote_session_id;
    let token = session.backend_token.clone();
    let actual_sec = session.elapsed_seconds
        + session
            .started_at_instant
            .map(|i| i.elapsed().as_secs() as u32)
            .unwrap_or(0);
    let div_count = session.diversion_count;
    session.elapsed_seconds = actual_sec;
    session.started_at_instant = None;

    if was_active && !sess_id.is_empty() {
        let _ = state.db.update_session_end(
            &sess_id,
            &Utc::now().to_rfc3339(),
            actual_sec,
            "completed",
            div_count,
        );
    }

    if was_active {
        if let (Some(t), Some(rid)) = (token, remote_id) {
            let client = state.sync_client.clone();
            tauri::async_runtime::spawn(async move {
                let _ = client.stop_remote_session(&t, rid).await;
            });
        }
    }

    hide_all_overlays(app);

    let snapshot = session.clone();
    drop(session);

    let _ = app.emit("session-changed", &snapshot);
    snapshot
}

#[tauri::command]
async fn start_focus_session(
    app: AppHandle,
    state: State<'_, AppState>,
    task_title: String,
    duration_minutes: u32,
    token: Option<String>,
) -> Result<SessionState, String> {
    // Reserve lifecycle generation before awaiting remote creation
    let target_gen = state.session_generation.fetch_add(1, Ordering::SeqCst) + 1;

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

    // Discard transition if session lifecycle changed while waiting for remote creation
    if state.session_generation.load(Ordering::SeqCst) != target_gen {
        if let (Some(tok), Some(rid)) = (token.clone(), remote_id) {
            let client = state.sync_client.clone();
            tauri::async_runtime::spawn(async move {
                let _ = client.stop_remote_session(&tok, rid).await;
            });
        }
        return Err("Session start superseded by another action".to_string());
    }

    internal_start_session(&app, &state, clean_title, duration_minutes, remote_id, token)
}

#[tauri::command]
async fn end_focus_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionState, String> {
    let snapshot = internal_end_session(&app, &state);
    Ok(snapshot)
}

#[tauri::command]
fn trigger_barrier(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    session.is_overlay_visible = true;

    show_all_overlays(&app);

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[tauri::command]
fn dismiss_barrier(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    session.is_overlay_visible = false;
    session.diversion_count += 1;

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

    state
        .rules
        .lock()
        .unwrap()
        .grant_bypass_grace(Duration::from_secs(60));

    hide_all_overlays(&app);

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[tauri::command]
async fn record_barrier_bypass(
    app: AppHandle,
    state: State<'_, AppState>,
    token: Option<String>,
    reason: Option<String>,
) -> Result<Option<sync::DiversionPenaltyResult>, String> {
    let (sess_id, remote_id, auth_token, is_active) = {
        let session = state.session.lock().unwrap();
        (
            session.session_id.clone(),
            session.remote_session_id,
            token.or_else(|| session.backend_token.clone()),
            session.is_focus_active,
        )
    };

    if !is_active || sess_id.is_empty() {
        return Ok(None);
    }

    let trigger_reason = reason.unwrap_or_else(|| "manual_bypass".to_string());
    let mut penalty_res = None;

    if let Some(tok) = auth_token {
        if !tok.trim().is_empty() {
            let res = state
                .sync_client
                .record_diversion_penalty(&tok, remote_id, &trigger_reason, 15.0)
                .await?;
            penalty_res = Some(res);
        }
    }

    // Only mutate session, log SQLite diversion, and grant grace after penalty call succeeds
    // Recheck session ID to ensure the session hasn't changed or ended during the remote await
    let mut session_changed = false;
    let mut snapshot_opt = None;
    {
        let mut session = state.session.lock().unwrap();
        if session.session_id == sess_id && session.is_focus_active {
            session.is_overlay_visible = false;
            session.diversion_count += 1;
            session_changed = true;
            snapshot_opt = Some(session.clone());
        }
    }

    if session_changed {
        let div_record = db::DiversionEventRecord {
            id: format!("div_{}", Utc::now().timestamp_millis()),
            session_id: sess_id,
            timestamp: Utc::now().to_rfc3339(),
            trigger_type: trigger_reason,
            detected_title: None,
            bypassed: true,
        };
        let _ = state.db.record_diversion(&div_record);

        state
            .rules
            .lock()
            .unwrap()
            .grant_bypass_grace(Duration::from_secs(60));

        if let Some(snapshot) = snapshot_opt {
            let _ = app.emit("session-changed", &snapshot);
        }
    }

    Ok(penalty_res)
}

#[tauri::command]
fn dismiss_overlay_window(app: AppHandle) -> Result<(), String> {
    hide_all_overlays(&app);
    Ok(())
}

#[tauri::command]
fn activate_tactical_shield(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state.session.lock().unwrap();
    if session.shield_spent {
        return Err("Tactical shield already expended for this session".to_string());
    }

    let until = Utc::now().timestamp_millis() + 300_000;
    session.tactical_shield_until = Some(until);
    session.shield_spent = true;
    session.is_overlay_visible = false;
    let snapshot = session.clone();
    drop(session);

    hide_all_overlays(&app);
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[tauri::command]
async fn sync_remote_session(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<Option<sync::RemoteSessionState>, String> {
    if token.trim().is_empty() {
        return Ok(None);
    }

    let sync_gen = state.session_generation.load(Ordering::SeqCst);

    let remote_state = state
        .sync_client
        .fetch_active_session_snapshot(&token)
        .await?;

    // Discard response if superseded by local action during fetch
    if state.session_generation.load(Ordering::SeqCst) != sync_gen {
        return Ok(None);
    }

    if let Some(ref remote) = remote_state {
        let mut session = state.session.lock().unwrap();
        if state.session_generation.load(Ordering::SeqCst) != sync_gen {
            return Ok(None);
        }

        session.backend_token = Some(token.clone());

        if remote.is_active {
            let mut changed = false;
            let mut newly_created_in_tick = false;

            if let Some(ref phase) = remote.current_phase {
                let should_be_active = phase.kind == "focus" && phase.status == "active";
                if session.is_focus_active != should_be_active {
                    session.is_focus_active = should_be_active;
                    if !should_be_active {
                        // Pause elapsed focus timer when transitioning to break
                        if let Some(instant) = session.started_at_instant.take() {
                            session.elapsed_seconds += instant.elapsed().as_secs() as u32;
                        }
                        session.is_overlay_visible = false;
                        hide_all_overlays(&app);
                    } else {
                        // Resumed focus
                        session.started_at_instant = Some(std::time::Instant::now());
                    }
                    if should_be_active && session.session_id.is_empty() && session.remote_session_id == remote.session_id {
                        let new_id = format!("sync_{}", Utc::now().timestamp_millis());
                        session.session_id = new_id.clone();
                        session.elapsed_seconds = 0;
                        session.started_at_instant = Some(std::time::Instant::now());
                        session.shield_spent = false;
                        session.tactical_shield_until = None;
                        session.diversion_count = 0;
                        let record = db::FocusSessionRecord {
                            id: new_id,
                            task_id: remote.task_id,
                            task_title: remote.activity_label.clone().unwrap_or_else(|| "Remote Pomodoro".to_string()),
                            started_at: Utc::now().to_rfc3339(),
                            ended_at: None,
                            target_duration_minutes: phase.minutes,
                            actual_duration_seconds: 0,
                            status: "running".to_string(),
                            diversion_count: 0,
                            synced_to_backend: true,
                        };
                        let _ = state.db.insert_session(&record);
                        newly_created_in_tick = true;
                    }
                    changed = true;
                }
                if session.duration_minutes != phase.minutes && phase.minutes > 0 {
                    session.duration_minutes = phase.minutes;
                    changed = true;
                }
            }
            if let Some(ref label) = remote.activity_label {
                if session.task_title != *label {
                    session.task_title = label.clone();
                    changed = true;
                }
            }
            if session.remote_session_id != remote.session_id {
                // If previous session was active and not newly created in this tick, finalize its local record
                if !newly_created_in_tick && session.is_focus_active && !session.session_id.is_empty() {
                    let ended_at = Utc::now().to_rfc3339();
                    let actual_sec = session.elapsed_seconds
                        + session
                            .started_at_instant
                            .map(|i| i.elapsed().as_secs() as u32)
                            .unwrap_or(0);
                    let _ = state.db.update_session_end(
                        &session.session_id,
                        &ended_at,
                        actual_sec,
                        "completed",
                        session.diversion_count,
                    );
                }
                let new_id = format!("sync_{}", Utc::now().timestamp_millis());
                session.session_id = new_id.clone();
                session.remote_session_id = remote.session_id;
                session.elapsed_seconds = 0;
                session.started_at_instant = if session.is_focus_active {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                session.shield_spent = false;
                session.tactical_shield_until = None;
                session.diversion_count = 0;
                let target_min = remote.current_phase.as_ref().map(|p| p.minutes).unwrap_or(session.duration_minutes);
                let record = db::FocusSessionRecord {
                    id: new_id,
                    task_id: remote.task_id,
                    task_title: remote.activity_label.clone().unwrap_or_else(|| session.task_title.clone()),
                    started_at: Utc::now().to_rfc3339(),
                    ended_at: None,
                    target_duration_minutes: target_min,
                    actual_duration_seconds: 0,
                    status: "running".to_string(),
                    diversion_count: 0,
                    synced_to_backend: true,
                };
                let _ = state.db.insert_session(&record);
                changed = true;
            }
            if changed {
                let snapshot = session.clone();
                let _ = app.emit("session-changed", &snapshot);
            }
        } else if session.is_focus_active && session.remote_session_id.is_some() {
            // Synchronized remote session ended -> finalize local row and drop to Free Time
            session.is_focus_active = false;
            session.is_overlay_visible = false;
            let sess_id = session.session_id.clone();
            let ended_at = Utc::now().to_rfc3339();
            let actual_sec = session.elapsed_seconds
                + session
                    .started_at_instant
                    .map(|i| i.elapsed().as_secs() as u32)
                    .unwrap_or(0);
            let div_count = session.diversion_count;
            session.elapsed_seconds = actual_sec;
            session.started_at_instant = None;
            session.remote_session_id = None;
            let _ = state.db.update_session_end(&sess_id, &ended_at, actual_sec, "completed", div_count);

            hide_all_overlays(&app);
            let snapshot = session.clone();
            let _ = app.emit("session-changed", &snapshot);
        }
    }

    Ok(remote_state)
}

#[tauri::command]
fn toggle_task_hud(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
    }
    let _ = app.emit("toggle-task-hud", ());
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(main) = app.get_webview_window("main") {
                            let _ = main.show();
                        }
                        let _ = app.emit("toggle-task-hud", ());
                    }
                })
                .build(),
        )
        .setup(|app| {
            let app_data = app.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let _ = std::fs::create_dir_all(&app_data);
            let db_path = app_data.join("task_buddy_desktop.db");

            let db_instance = db::Database::new(&db_path).expect("Failed to initialize SQLite database");
            let sync_client = sync::BackendSyncClient::default();
            let matcher = ai::ContentMatcher::new();
            let rules_engine = Arc::new(Mutex::new(monitor::RulesEngine::new()));

            let app_state = AppState {
                session: Arc::new(Mutex::new(SessionState::default())),
                db: db_instance.clone(),
                sync_client,
                matcher: matcher.clone(),
                session_generation: Arc::new(AtomicU64::new(0)),
                rules: rules_engine.clone(),
            };

            let session_ref = app_state.session.clone();
            let db_ref = db_instance.clone();
            let matcher_ref = matcher.clone();
            let rules_ref = rules_engine.clone();

            app.manage(app_state);

            // Register global hotkey Ctrl+Alt+Space for instant task glance HUD
            if let Ok(shortcut) = "ctrl+alt+space".parse::<Shortcut>() {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    eprintln!("[shortcut] Failed to register Ctrl+Alt+Space: {e:?}");
                }
            }

            // Spawn secondary overlays for additional connected monitors
            // Note: Secondary overlays are enumerated for displays connected at launch. Displays connected after launch require an app restart to provision.
            let primary_pos = app.primary_monitor().ok().flatten().map(|m| *m.position());
            if let Ok(monitors) = app.available_monitors() {
                let mut secondary_idx = 1;
                for monitor in monitors {
                    if Some(*monitor.position()) == primary_pos {
                        continue;
                    }
                    let label = format!("overlay_{}", secondary_idx);
                    let title = format!("Task Buddy Barrier - Display {}", secondary_idx + 1);
                    secondary_idx += 1;

                    match WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
                        .title(title)
                        .transparent(true)
                        .decorations(false)
                        .always_on_top(true)
                        .visible(false)
                        .skip_taskbar(true)
                        .build()
                    {
                        Ok(window) => {
                            if let Err(e) = window.set_position(tauri::Position::Physical(*monitor.position())) {
                                eprintln!("[overlay] Failed to position secondary overlay on {label}: {e:?}");
                            }
                            if let Err(e) = window.set_fullscreen_on_monitor(tauri::PhysicalPosition::new(
                                monitor.position().x as f64,
                                monitor.position().y as f64,
                            )) {
                                eprintln!("[overlay] Failed to fullscreen secondary overlay on {label}: {e:?}");
                            }
                        }
                        Err(e) => {
                            eprintln!("[overlay] Failed to spawn secondary overlay on {label}: {e:?}");
                        }
                    }
                }
            }

            // Spawn Background OS Monitor Thread with dynamic SQLite rules and AI matcher
            let monitor_handle = app.handle().clone();
            let monitor_session = session_ref.clone();
            let monitor_db = db_ref.clone();
            let monitor_matcher = matcher_ref.clone();
            let monitor_rules = rules_ref.clone();

            std::thread::spawn(move || {
                unsafe {
                    let _ = windows::Win32::System::WinRT::RoInitialize(
                        windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                    );
                }

                let mut last_rules_update = std::time::Instant::now();

                // Initial rules load
                if let Ok(initial_rules) = monitor_db.get_rules() {
                    monitor_rules.lock().unwrap().update_rules(&initial_rules);
                }

                loop {
                    std::thread::sleep(Duration::from_millis(500));

                    // Refresh custom rules from SQLite every 5 seconds
                    if last_rules_update.elapsed() > Duration::from_secs(5) {
                        if let Ok(updated_rules) = monitor_db.get_rules() {
                            monitor_rules.lock().unwrap().update_rules(&updated_rules);
                        }
                        last_rules_update = std::time::Instant::now();
                    }

                    let (is_focus_active, active_task_title, shield_until) = {
                        let lock = monitor_session.lock().unwrap();
                        (lock.is_focus_active, lock.task_title.clone(), lock.tactical_shield_until)
                    };

                    if let Some(until) = shield_until {
                        let now_ms = Utc::now().timestamp_millis();
                        if now_ms < until {
                            monitor_rules
                                .lock()
                                .unwrap()
                                .grant_tactical_shield(Duration::from_millis((until - now_ms) as u64));
                        }
                    }

                    let window = monitor::get_active_window_info();
                    let media = monitor::get_current_media_status();

                    // Evaluate content alignment using non-blocking matcher (cache + background Groq + heuristic)
                    let is_media_aligned = if media.has_active_media && !media.title.is_empty() && is_focus_active {
                        monitor_matcher.evaluate_monitor_alignment(&active_task_title, &media.title).aligned
                    } else {
                        true
                    };

                    let eval = monitor_rules
                        .lock()
                        .unwrap()
                        .evaluate(is_focus_active, &window, &media, is_media_aligned);
                    if let monitor::EvaluationResult::TriggerBarrier { reason } = eval {
                        let mut trigger_overlay = false;
                        let mut snapshot_opt = None;
                        let mut sess_id_opt = None;
                        {
                            let mut session = monitor_session.lock().unwrap();
                            if session.is_focus_active && !session.is_overlay_visible {
                                session.is_overlay_visible = true;
                                trigger_overlay = true;
                                snapshot_opt = Some(session.clone());
                                sess_id_opt = Some(session.session_id.clone());
                            }
                        }
                        if trigger_overlay {
                            // Synchronize overlay presentation across all monitors after dropping session lock to prevent deadlock
                            show_all_overlays(&monitor_handle);
                            if let Some(sess_id) = sess_id_opt {
                                let div = db::DiversionEventRecord {
                                    id: format!("div_{}", Utc::now().timestamp_millis()),
                                    session_id: sess_id,
                                    timestamp: Utc::now().to_rfc3339(),
                                    trigger_type: reason.to_string(),
                                    detected_title: Some(window.title.clone()),
                                    bypassed: false,
                                };
                                let _ = monitor_db.record_diversion(&div);
                            }
                            if let Some(snapshot) = snapshot_opt {
                                let _ = monitor_handle.emit("session-changed", &snapshot);
                            }
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
                        state.session_generation.fetch_add(1, Ordering::SeqCst);
                        let _ = internal_start_session(app, &state, "Focus Session (25m)".to_string(), 25, None, None);
                    }
                    "start_50" => {
                        let state = app.state::<AppState>();
                        state.session_generation.fetch_add(1, Ordering::SeqCst);
                        let _ = internal_start_session(app, &state, "Deep Focus (50m)".to_string(), 50, None, None);
                    }
                    "end_session" => {
                        let state = app.state::<AppState>();
                        let _ = internal_end_session(app, &state);
                    }
                    "toggle_overlay" => {
                        let is_visible = app
                            .get_webview_window("overlay")
                            .and_then(|o| o.is_visible().ok())
                            .unwrap_or(false);
                        if is_visible {
                            hide_all_overlays(app);
                        } else {
                            show_all_overlays(app);
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
            record_barrier_bypass,
            dismiss_overlay_window,
            activate_tactical_shield,
            sync_remote_session,
            toggle_task_hud,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
