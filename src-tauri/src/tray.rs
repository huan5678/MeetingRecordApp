//! System tray: recording-status icon + quick controls (PRD §3.7 stories
//! 37/38). The tray is the always-on entry point — the user starts/stops a
//! recording and reopens the main window from here without leaving their
//! meeting.
//!
//! Tauri 2 builds the tray imperatively at `setup` time (the static
//! `app.trayIcon` in `tauri.conf.json` only declares the icon/tooltip). We
//! build a menu, register a menu-event handler that maps each item to a
//! [`crate::commands`] action, and expose [`build_tray`] for `run` to call.
//!
//! The menu item set is intentionally small and mirrors the floating mini-panel:
//! Start / Pause / Resume / Stop, Show window, Quit. We rebuild the menu's
//! enabled-state from the recording phase via [`tray_menu_for`] so the user
//! can't, e.g., "Pause" when idle.

use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

use crate::commands::{AppState, RecordingPhase};

/// Stable ids for the tray menu items, referenced in the event handler.
pub mod ids {
    pub const START: &str = "tray_start";
    pub const PAUSE: &str = "tray_pause";
    pub const RESUME: &str = "tray_resume";
    pub const STOP: &str = "tray_stop";
    pub const SHOW: &str = "tray_show";
    pub const QUIT: &str = "tray_quit";
}

/// The label shown for an item, plus whether it should be enabled given the
/// current recording phase. Pure so it can be unit-tested without a running app.
pub fn item_enabled(id: &str, phase: RecordingPhase) -> bool {
    use RecordingPhase::*;
    match id {
        ids::START => phase == Idle,
        ids::PAUSE => phase == Recording,
        ids::RESUME => phase == Paused,
        ids::STOP => matches!(phase, Recording | Paused),
        ids::SHOW | ids::QUIT => true,
        _ => true,
    }
}

/// The tooltip text reflecting the current phase (shown on hover).
pub fn tooltip_for(phase: RecordingPhase) -> &'static str {
    match phase {
        RecordingPhase::Idle => "MeetingRecordApp — idle",
        RecordingPhase::Recording => "MeetingRecordApp — recording",
        RecordingPhase::Paused => "MeetingRecordApp — paused",
        RecordingPhase::Stopping => "MeetingRecordApp — stopping…",
    }
}

/// Build the tray menu for a given phase, with each control's enabled-state
/// derived from [`item_enabled`].
fn build_menu<R: Runtime>(app: &AppHandle<R>, phase: RecordingPhase) -> tauri::Result<Menu<R>> {
    let start = MenuItem::with_id(app, ids::START, "Start recording", item_enabled(ids::START, phase), None::<&str>)?;
    let pause = MenuItem::with_id(app, ids::PAUSE, "Pause", item_enabled(ids::PAUSE, phase), None::<&str>)?;
    let resume = MenuItem::with_id(app, ids::RESUME, "Resume", item_enabled(ids::RESUME, phase), None::<&str>)?;
    let stop = MenuItem::with_id(app, ids::STOP, "Stop recording", item_enabled(ids::STOP, phase), None::<&str>)?;
    let show = MenuItem::with_id(app, ids::SHOW, "Show window", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, ids::QUIT, "Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&start, &pause, &resume, &stop, &show, &quit])
}

/// Current recording phase from app state (defaults to `Idle` if unset / locked).
fn current_phase<R: Runtime>(app: &AppHandle<R>) -> RecordingPhase {
    app.try_state::<AppState>()
        .and_then(|s| s.recording.lock().ok().map(|r| r.phase.unwrap_or(RecordingPhase::Idle)))
        .unwrap_or(RecordingPhase::Idle)
}

/// Show + focus the main window (used by the tray "Show window" item and a
/// left-click on the tray icon).
fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// Build the system tray and wire its menu + click handlers. Call once from
/// `run`'s `setup`. The heavy lifting of each control is delegated to the same
/// command functions the frontend invokes, so behaviour is identical whether
/// the user clicks a tray item or a UI button.
pub fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcon<R>> {
    let phase = current_phase(app);
    let menu = build_menu(app, phase)?;

    let tray = TrayIconBuilder::with_id("main-tray")
        .tooltip(tooltip_for(phase))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            match id {
                ids::SHOW => show_main_window(app),
                ids::QUIT => app.exit(0),
                ids::START | ids::PAUSE | ids::RESUME | ids::STOP => {
                    handle_recording_action(app, id);
                }
                _ => {}
            }
            // Refresh the menu so enabled-states track the new phase.
            refresh_tray(app);
        })
        .on_tray_icon_event(|tray, event| {
            // Left click reopens the main window (menuOnLeftClick is false, so a
            // left click won't pop the menu).
            if let TrayIconEvent::Click { .. } = event {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(tray)
}

/// Apply a recording control from the tray by mutating [`AppState`] directly —
/// the same state the `#[tauri::command]` handlers own. We don't call the
/// command fns (they need a `State` extractor / async runtime); instead we make
/// the identical phase transition so tray + UI stay consistent.
fn handle_recording_action<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(mut rec) = state.recording.lock() else {
        return;
    };
    let phase = rec.phase.unwrap_or(RecordingPhase::Idle);
    match (id, phase) {
        (ids::PAUSE, RecordingPhase::Recording) => rec.phase = Some(RecordingPhase::Paused),
        (ids::RESUME, RecordingPhase::Paused) => rec.phase = Some(RecordingPhase::Recording),
        (ids::STOP, RecordingPhase::Recording | RecordingPhase::Paused) => {
            rec.phase = Some(RecordingPhase::Idle);
            rec.meeting_id = None;
            rec.elapsed_seconds = 0;
        }
        // START from the tray is a UI affordance: it surfaces the main window so
        // the user picks devices, rather than starting a headless capture.
        (ids::START, RecordingPhase::Idle) => {
            drop(rec);
            show_main_window(app);
        }
        _ => {}
    }
}

/// Rebuild the tray menu + tooltip to reflect the current phase. Cheap; called
/// after any tray-driven transition. Safe to call when no tray exists yet.
pub fn refresh_tray<R: Runtime>(app: &AppHandle<R>) {
    let phase = current_phase(app);
    if let Some(tray) = app.tray_by_id("main-tray") {
        if let Ok(menu) = build_menu(app, phase) {
            let _ = tray.set_menu(Some(menu));
        }
        let _ = tray.set_tooltip(Some(tooltip_for(phase)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_enabled_only_when_idle() {
        assert!(item_enabled(ids::START, RecordingPhase::Idle));
        assert!(!item_enabled(ids::START, RecordingPhase::Recording));
        assert!(!item_enabled(ids::START, RecordingPhase::Paused));
    }

    #[test]
    fn pause_resume_stop_gating() {
        assert!(item_enabled(ids::PAUSE, RecordingPhase::Recording));
        assert!(!item_enabled(ids::PAUSE, RecordingPhase::Paused));
        assert!(item_enabled(ids::RESUME, RecordingPhase::Paused));
        assert!(!item_enabled(ids::RESUME, RecordingPhase::Recording));
        assert!(item_enabled(ids::STOP, RecordingPhase::Recording));
        assert!(item_enabled(ids::STOP, RecordingPhase::Paused));
        assert!(!item_enabled(ids::STOP, RecordingPhase::Idle));
    }

    #[test]
    fn show_and_quit_always_enabled() {
        for phase in [
            RecordingPhase::Idle,
            RecordingPhase::Recording,
            RecordingPhase::Paused,
            RecordingPhase::Stopping,
        ] {
            assert!(item_enabled(ids::SHOW, phase));
            assert!(item_enabled(ids::QUIT, phase));
        }
    }

    #[test]
    fn tooltip_tracks_phase() {
        assert!(tooltip_for(RecordingPhase::Recording).contains("recording"));
        assert!(tooltip_for(RecordingPhase::Paused).contains("paused"));
        assert!(tooltip_for(RecordingPhase::Idle).contains("idle"));
    }
}
