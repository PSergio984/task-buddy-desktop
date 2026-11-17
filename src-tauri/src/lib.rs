use std::sync::{Arc, Mutex};
use tauri::{
    menu::{MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State,
};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    pub is_focus_active: bool,
    pub task_title: String,
    pub duration_minutes: u32,
    pub elapsed_seconds: u32,
    pub is_overlay_visible: bool,
}

pub struct AppState {
    pub session: Arc<Mutex<SessionState>>,
}

#[tauri::command]
fn get_session_status(state: State<'_, AppState>) -> SessionState {
    state.session.lock().unwrap().clone()
}

#[tauri::command]
fn start_focus_session(
    app: AppHandle,
    state: State<'_, AppState>,
    task_title: String,
    duration_minutes: u32,
) -> Result<SessionState, String> {
    let mut session = state.session.lock().unwrap();
    session.is_focus_active = true;
    session.task_title = if task_title.trim().is_empty() {
        "Deep Work".to_string()
    } else {
        task_title
    };
    session.duration_minutes = duration_minutes;
    session.elapsed_seconds = 0;
    session.is_overlay_visible = false;

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn end_focus_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionState, String> {
    let mut session = state.session.lock().unwrap();
    session.is_focus_active = false;
    session.is_overlay_visible = false;

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

    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.hide();
    }

    let snapshot = session.clone();
    let _ = app.emit("session-changed", &snapshot);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState {
        session: Arc::new(Mutex::new(SessionState::default())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_session_status,
            start_focus_session,
            end_focus_session,
            trigger_barrier,
            dismiss_barrier,
        ])
        .setup(|app| {
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
