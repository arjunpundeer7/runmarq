//! System tray: a glanceable summary of active runs and quick controls.
//!
//! ```text
//! runmarq
//! 2 agents working
//! Claude Code · About 6–10 min remaining
//! Codex · Waiting for approval
//! ```

use crate::AppState;
use runmarq_core::api::RunView;
use runmarq_core::forecast::{eta_line, ForecastStatus};
use runmarq_core::run::RunState;
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "runmarq";

pub fn run_line(r: &RunView) -> String {
    let agent = r.agent.display_name();
    let status = match r.state {
        RunState::WaitingForUser => "Waiting for approval".to_string(),
        RunState::WaitingForExternalResource => "Waiting on a resource".to_string(),
        RunState::Unknown => "Status unknown".to_string(),
        _ => match &r.forecast {
            Some(f) if f.status == ForecastStatus::Available => match f.words() {
                Some(w) => eta_line(&w),
                None => "Estimating".into(),
            },
            Some(f) if f.status == ForecastStatus::InsufficientData => {
                "Working · not enough information yet".into()
            }
            _ => "Estimating".into(),
        },
    };
    match &r.repository_name {
        Some(repo) => format!("{agent} ({repo}) · {status}"),
        None => format!("{agent} · {status}"),
    }
}

pub fn summary(runs: &[RunView]) -> String {
    let working = runs.iter().filter(|r| r.state.is_working()).count();
    let waiting = runs
        .iter()
        .filter(|r| r.state == RunState::WaitingForUser)
        .count();
    match (working, waiting) {
        (0, 0) => "No active runs".into(),
        (w, 0) => format!("{w} {} working", if w == 1 { "agent" } else { "agents" }),
        (0, n) => format!("{n} waiting for you"),
        (w, n) => format!("{w} working, {n} waiting for you"),
    }
}

fn build_menu(app: &AppHandle, state: &AppState) -> tauri::Result<Menu<Wry>> {
    let runs: Vec<RunView> = state.runs.lock().unwrap().values().cloned().collect();
    let quiet = state
        .prefs
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.quiet_mode)
        .unwrap_or(false);
    let widget = app
        .get_webview_window("widget")
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false);
    let connected = state.connected.load(std::sync::atomic::Ordering::Relaxed);

    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "title",
        "runmarq",
        false,
        None::<&str>,
    )?)?;
    let headline = if connected {
        summary(&runs)
    } else {
        "Collector offline".into()
    };
    menu.append(&MenuItem::with_id(
        app,
        "summary",
        headline,
        false,
        None::<&str>,
    )?)?;
    for (i, r) in runs.iter().take(8).enumerate() {
        menu.append(&MenuItem::with_id(
            app,
            format!("run-{i}"),
            run_line(r),
            false,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open runmarq",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "widget",
        if widget {
            "Hide compact panel"
        } else {
            "Show compact panel"
        },
        true,
        None::<&str>,
    )?)?;
    menu.append(&CheckMenuItem::with_id(
        app,
        "quiet",
        "Quiet mode",
        connected,
        quiet,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit runmarq",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

pub fn refresh(app: &AppHandle) {
    let state: tauri::State<Arc<AppState>> = app.state();
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, &state) {
        let _ = tray.set_menu(Some(menu));
    }
    let runs: Vec<RunView> = state.runs.lock().unwrap().values().cloned().collect();
    let tip = if runs.is_empty() {
        "runmarq · No active runs".to_string()
    } else {
        format!("runmarq · {}", summary(&runs))
    };
    let _ = tray.set_tooltip(Some(tip));
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state: tauri::State<Arc<AppState>> = app.state();
    let menu = build_menu(app, &state)?;
    let icon = app.default_window_icon().cloned().expect("bundle icon");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("runmarq")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::show_main(app),
            "widget" => {
                let visible = app
                    .get_webview_window("widget")
                    .map(|w| w.is_visible().unwrap_or(false))
                    .unwrap_or(false);
                crate::set_widget(app, !visible);
                refresh(app);
            }
            "quiet" => crate::toggle_quiet(app),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        assert_eq!(summary(&[]), "No active runs");
    }
}
