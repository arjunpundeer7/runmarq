//! runmarq desktop: native side of the Tauri app.

mod collector;
mod commands;
mod tray;

use collector::Collector;
use runmarq_core::api::{Preferences, RunView, StreamMessage};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

pub struct AppState {
    pub collector: Collector,
    pub runs: Mutex<BTreeMap<String, RunView>>,
    pub prefs: Mutex<Option<Preferences>>,
    pub connected: AtomicBool,
    pub quitting: AtomicBool,
}

#[derive(Clone, Serialize)]
struct Connection {
    connected: bool,
    error: Option<String>,
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn set_widget(app: &AppHandle, visible: bool) {
    let on_top = app
        .state::<Arc<AppState>>()
        .prefs
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.widget_always_on_top)
        .unwrap_or(false);
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.set_always_on_top(on_top);
        let _ = if visible { w.show() } else { w.hide() };
        return;
    }
    if !visible {
        return;
    }
    let built = WebviewWindowBuilder::new(app, "widget", WebviewUrl::App("widget.html".into()))
        .title("runmarq")
        .inner_size(300.0, 220.0)
        .min_inner_size(240.0, 48.0)
        .decorations(false)
        .resizable(true)
        .skip_taskbar(true)
        .always_on_top(on_top)
        .build();
    if let Err(e) = built {
        tracing::warn!("could not open the compact panel: {e}");
    }
}

pub fn apply_preferences(app: &AppHandle, p: &Preferences) {
    let state = app.state::<Arc<AppState>>();
    *state.prefs.lock().unwrap() = Some(p.clone());
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.set_always_on_top(p.widget_always_on_top);
        if !p.compact_widget {
            let _ = w.hide();
        }
    } else if p.compact_widget {
        set_widget(app, true);
    }
    tray::refresh(app);
}

pub fn toggle_quiet(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<Arc<AppState>>();
        let current: Result<Preferences, String> =
            state.collector.call(|c| c.get("/v1/preferences"));
        if let Ok(mut p) = current {
            p.quiet_mode = !p.quiet_mode;
            if let Ok(saved) = state
                .collector
                .call(|c| c.send::<_, Preferences>("PUT", "/v1/preferences", &p))
            {
                apply_preferences(&app, &saved);
                let _ = app.emit(
                    "runmarq://stream",
                    StreamMessage::PreferencesChanged { preferences: saved },
                );
            }
        }
    });
}

/// Quitting stops the collector too. Hooks keep working: events are spooled
/// to disk and replayed with their original timestamps on the next start.
pub fn quit(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();
    state.quitting.store(true, Ordering::Relaxed);
    state.collector.stop();
    app.exit(0);
}

fn handle(app: &AppHandle, state: &AppState, m: &StreamMessage) {
    let _ = app.emit("runmarq://stream", m);
    match m {
        StreamMessage::Snapshot { runs } => {
            *state.runs.lock().unwrap() = runs.iter().map(|r| (r.id.clone(), r.clone())).collect();
        }
        StreamMessage::RunUpdated { run } => {
            let mut runs = state.runs.lock().unwrap();
            if run.state.is_terminal() {
                runs.remove(&run.id);
            } else {
                runs.insert(run.id.clone(), run.clone());
            }
        }
        StreamMessage::Notification { notification } => {
            let sound = state
                .prefs
                .lock()
                .unwrap()
                .as_ref()
                .is_none_or(|p| p.notification_sound);
            let mut toast = app
                .notification()
                .builder()
                .title(&notification.title)
                .body(&notification.body);
            if sound {
                toast = toast.sound("Default");
            }
            if let Err(e) = toast.show() {
                tracing::warn!("notification failed: {e}");
            }
        }
        StreamMessage::PreferencesChanged { preferences } => {
            apply_preferences(app, preferences);
            return;
        }
    }
    tray::refresh(app);
}

fn stream_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<Arc<AppState>>().inner().clone();
        let mut backoff = 1u64;
        loop {
            if state.quitting.load(Ordering::Relaxed) {
                break;
            }
            match state.collector.ensure_running() {
                Ok(client) => {
                    state.connected.store(true, Ordering::Relaxed);
                    let _ = app.emit(
                        "runmarq://connection",
                        Connection {
                            connected: true,
                            error: None,
                        },
                    );
                    if let Ok(p) = client.get::<Preferences>("/v1/preferences") {
                        apply_preferences(&app, &p);
                    }
                    backoff = 1;
                    let res = client.stream(|m| {
                        handle(&app, &state, &m);
                        !state.quitting.load(Ordering::Relaxed)
                    });
                    state.connected.store(false, Ordering::Relaxed);
                    let error = Some(
                        res.err()
                            .map(|e| e.to_string())
                            .unwrap_or_else(|| "the collector connection closed".into()),
                    );
                    let _ = app.emit(
                        "runmarq://connection",
                        Connection {
                            connected: false,
                            error,
                        },
                    );
                    tray::refresh(&app);
                }
                Err(e) => {
                    state.connected.store(false, Ordering::Relaxed);
                    let _ = app.emit(
                        "runmarq://connection",
                        Connection {
                            connected: false,
                            error: Some(e),
                        },
                    );
                    tray::refresh(&app);
                }
            }
            std::thread::sleep(Duration::from_secs(backoff));
            backoff = (backoff * 2).min(10);
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (paths, moved) =
        runmarq_collector::migrate::resolve_migrating(collector::bundled_cli().as_deref());
    let _ = runmarq_collector::lifecycle::ensure_dirs(&paths);
    let _log = runmarq_collector::init_file_logging(&paths, "desktop");
    if let Some(m) = moved {
        tracing::info!(
            from = %m.from.display(),
            to = %m.to.display(),
            rewritten = m.rewritten.len(),
            problems = ?m.problems,
            "moved data from the pre-rename folder"
        );
    }
    let hidden = std::env::args().any(|a| a == "--hidden");

    let state = Arc::new(AppState {
        collector: Collector::new(paths),
        runs: Mutex::new(BTreeMap::new()),
        prefs: Mutex::new(None),
        connected: AtomicBool::new(false),
        quitting: AtomicBool::new(false),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::active_runs,
            commands::history,
            commands::run_detail,
            commands::repositories,
            commands::insights,
            commands::usage,
            commands::diagnostics,
            commands::get_preferences,
            commands::set_preferences,
            commands::integrations,
            commands::integration_plan,
            commands::integration_install,
            commands::integration_remove,
            commands::export_csv,
            commands::delete_history,
            commands::restart_collector,
            commands::open_path,
            commands::terminal_command,
            commands::set_terminal_command,
            commands::set_widget_visible,
        ])
        .setup(move |app| {
            if let Err(e) = state.collector.prepare_cli() {
                tracing::error!("{e}");
            }
            tray::create(app.handle())?;
            stream_loop(app.handle().clone());
            if !hidden {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window keeps runmarq in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.app_handle().state::<Arc<AppState>>();
                if !state.quitting.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running runmarq");
}
