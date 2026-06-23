//! MeetingRecordApp library crate.
//!
//! The binary (`main.rs`) is a thin shim that calls [`run`]. Module agents
//! fill in each `pub mod` listed below; the Integrate phase wires the real
//! command set into [`run`].
//!
//! v1.0 scope (per docs/PRD.md): **Windows-only, audio-only**. Code that can
//! only work on Windows is gated with `#[cfg(target_os = "windows")]`; on other
//! platforms the same functions return a clear "unsupported on this platform
//! (v1.1)" error so the crate still compiles and links everywhere.

pub mod ai;
pub mod audio;
pub mod commands;
pub mod detection;
pub mod export;
pub mod models;
pub mod storage;
pub mod transcription;
pub mod tray;

/// Screen recording. v1.1 — NOT implemented in v1.0 (audio-only). Stub only.
pub mod video;

/// Error returned by platform-specific paths that are not available on the
/// current OS in v1.0. Module agents can convert into their own error types.
pub fn unsupported_platform(feature: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{feature} is unsupported on this platform in v1.0 (Windows-only); planned for v1.1"
    )
}

/// A trivial command so the app always has a working IPC endpoint; useful for a
/// frontend "is the backend alive?" probe.
#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

use tauri::Manager;

/// Build and run the Tauri application.
///
/// Wiring (Integrate phase):
/// - Resolve the per-OS app-data directory and [`commands::AppState::bootstrap`]
///   the SQLite DB + recordings file store; register it as managed state.
/// - Build the system tray ([`tray::build_tray`]) in `setup`.
/// - Register the full `#[tauri::command]` surface from [`commands`] (names must
///   match `src/lib/tauri.ts`'s `COMMANDS`).
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Per-OS app-data dir (e.g. %APPDATA%\com.meetingrecordapp.app on
            // Windows). Fall back to the current dir only if path resolution
            // fails, so the app still starts in a dev shell.
            let data_dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let state = commands::AppState::bootstrap(&data_dir)
                .map_err(|e| format!("failed to initialise app state: {e}"))?;
            app.manage(state);

            // Build the system tray (recording status + quick controls).
            tray::build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            // recording
            commands::start_recording,
            commands::stop_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::get_recording_status,
            // meetings
            commands::list_meetings,
            commands::get_meeting_detail,
            commands::delete_meeting,
            commands::update_meeting,
            commands::search_transcripts,
            // transcription
            commands::get_transcription_status,
            commands::retranscribe_meeting,
            commands::import_audio_meeting,
            commands::list_transcript_runs,
            commands::get_run_segments,
            commands::delete_transcript_run,
            commands::clear_transcripts,
            commands::delete_summary,
            commands::update_segment,
            // summary
            commands::generate_summary,
            commands::estimate_summary_cost,
            // export
            commands::export_meeting,
            // settings + devices + keychain
            commands::get_settings,
            commands::set_setting,
            commands::list_audio_devices,
            commands::set_api_key,
            commands::has_api_key,
            // storage
            commands::get_storage_usage,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_matches_cargo() {
        assert_eq!(app_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn unsupported_platform_message_mentions_v1_1() {
        let msg = unsupported_platform("System audio loopback").to_string();
        assert!(msg.contains("v1.1"));
        assert!(msg.contains("System audio loopback"));
    }
}
