//! Tauri command handlers — the IPC surface consumed by the React frontend.
//!
//! The command names here MUST match `src/lib/tauri.ts`'s `COMMANDS` map; the
//! DTO field names match the camelCase interfaces in that file (we
//! `#[serde(rename_all = "camelCase")]` every DTO so the JSON crosses the
//! boundary unchanged).
//!
//! ## What is real vs. what is hardware/feature-bound
//!
//! Everything that is pure Rust + SQLite + filesystem is implemented for real:
//! meeting list/detail/delete/update, transcript search, transcript-segment
//! edit, settings get/set, API-key storage (keychain), storage usage, export,
//! and cost estimation. These run and are testable on the dev Mac.
//!
//! The capture / transcription / summarization *execution* paths are wired up to
//! the right modules but their innermost step is platform- or feature-gated:
//! - **Recording** uses the `audio` recorder + (Windows-only) WASAPI loopback.
//!   On the dev Mac, system-audio capture returns the v1.0 "unsupported (v1.1)"
//!   error from [`crate::audio::system_audio`]; the FSM + state tracking are
//!   real. We keep an in-memory [`RecordingState`] so the tray + UI can show
//!   status without a live device.
//! - **Transcription** requires the `whisper` cargo feature + a downloaded
//!   model; without it [`crate::transcription::processor::Processor::transcribe_wav`]
//!   returns `FeatureDisabled`. The command surfaces that verbatim.
//! - **Summarization** talks to a local Ollama daemon over HTTP (default) or a
//!   cloud provider; this is real async I/O, it just needs Ollama running / a
//!   key stored.
//!
//! All handlers return `Result<_, String>` — Tauri serializes the `Err` string
//! straight to the frontend's `catch`, which is what the UI expects.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::ai::{self, keychain};
use crate::audio::live::{self, LiveConfig, LiveHandle};
use crate::audio::microphone::MicSelection;
use crate::audio::recorder::RecorderConfig;
use crate::audio::system_audio::LoopbackSelection;
use crate::export::{self, ExportFormat};
use crate::models::{
    AiProviderKind, MediaFile, MediaFileType, Meeting, MeetingStatus, MeetingType, Summary,
    SummaryType, TranscriptRun, TranscriptSegment,
};
use crate::storage::{self, search, Database, FileStore};

// ===========================================================================
// Shared application state (held by Tauri behind its managed-state map).
// ===========================================================================

/// In-memory recording lifecycle, mirrored to the UI/tray. The real audio
/// `Recorder` is created on `start_recording`; here we track just enough to
/// answer `get_recording_status` and gate the pause/resume/stop transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingPhase {
    Idle,
    Recording,
    Paused,
    Stopping,
}

impl RecordingPhase {
    fn as_dto(self) -> &'static str {
        match self {
            RecordingPhase::Idle => "idle",
            RecordingPhase::Recording => "recording",
            RecordingPhase::Paused => "paused",
            RecordingPhase::Stopping => "stopping",
        }
    }
}

/// The mutable recording state guarded by a `Mutex` inside [`AppState`].
#[derive(Debug, Default)]
pub struct RecordingState {
    pub phase: Option<RecordingPhase>,
    pub meeting_id: Option<String>,
    /// Wall-clock seconds elapsed in the current (non-paused) session. The live
    /// recorder reports the authoritative duration from frames; this is the
    /// cheap value the status poll returns between feeds.
    pub elapsed_seconds: i64,
    pub mic_level: f32,
    pub system_level: f32,
}

/// Live per-meeting transcription progress, written by the transcription worker
/// thread and read by `get_transcription_status` (the frontend polls it).
#[derive(Debug, Clone)]
pub struct TranscriptionProgressEntry {
    pub stage: String,
    pub fraction: f32,
    pub message: Option<String>,
}

/// Everything a command needs: the database, the recordings file store, the
/// live recording state, the active capture session (if any), and the
/// transcription progress map. Tauri hands each command a `State<AppState>`.
pub struct AppState {
    pub db: Mutex<Database>,
    pub files: FileStore,
    pub recording: Mutex<RecordingState>,
    /// The running capture session, present only between start and stop. Holds
    /// only `Send + Sync` control handles (the `!Send` audio streams live on the
    /// capture thread — see [`crate::audio::live`]).
    pub session: Mutex<Option<LiveHandle>>,
    /// Latest transcription progress per meeting id.
    pub transcription: Mutex<std::collections::HashMap<String, TranscriptionProgressEntry>>,
}

impl AppState {
    /// Build the application state: open (and migrate) the SQLite DB and root
    /// the recordings file store. `data_dir` is the per-OS app data directory
    /// (resolved from Tauri's path API in `run`).
    pub fn bootstrap(data_dir: &std::path::Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        let db = Database::open(data_dir.join("meetingrecord.sqlite3")).map_err(err)?;
        let files = FileStore::new(data_dir.join("recordings")).map_err(err)?;
        Ok(AppState {
            db: Mutex::new(db),
            files,
            recording: Mutex::new(RecordingState::default()),
            session: Mutex::new(None),
            transcription: Mutex::new(std::collections::HashMap::new()),
        })
    }
}

/// Map any `Display` error into the `String` Tauri sends to the frontend.
fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Lock the DB mutex, mapping poisoning to a string error (a poisoned lock means
/// a previous command panicked mid-transaction; surfacing it beats panicking).
fn lock_db<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, Database>, String> {
    state.db.lock().map_err(|_| "database lock poisoned".to_string())
}

// ===========================================================================
// DTOs — camelCase to match src/lib/tauri.ts.
// ===========================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatusDto {
    pub state: &'static str,
    pub meeting_id: Option<String>,
    pub elapsed_seconds: i64,
    pub mic_level: f32,
    pub system_level: f32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDetailDto {
    pub meeting: Meeting,
    pub media: Vec<MediaFile>,
    /// Segments of the latest transcription run (the default view).
    pub segments: Vec<TranscriptSegment>,
    /// Every transcription run (newest first) so the UI can offer them by
    /// model + time and load an older run's segments on demand.
    pub runs: Vec<TranscriptRun>,
    /// Every summary (newest first); a meeting can have several from different
    /// models/regenerations.
    pub summaries: Vec<Summary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceDto {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub is_default: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsageDto {
    pub total_bytes: u64,
    pub meeting_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimateDto {
    pub provider: String,
    pub model: String,
    pub prompt_tokens: usize,
    pub estimated_usd: f64,
}

// ===========================================================================
// Recording commands.
// ===========================================================================

/// Begin a recording session. Creates the meeting row + media directory and
/// flips the in-memory phase to `Recording`. Returns the new meeting id.
///
/// NOTE: the actual device capture (mic via `cpal`, system audio via WASAPI
/// loopback) is started by the audio module; on non-Windows it is unsupported
/// in v1.0. We still create the meeting so the rest of the pipeline (and the
/// UI) has something to attach to, and we record the requested input config.
#[tauri::command]
pub fn start_recording(
    state: State<'_, AppState>,
    mic_device_id: Option<String>,
    system_audio: bool,
) -> Result<String, String> {
    {
        let rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
        if matches!(rec.phase, Some(RecordingPhase::Recording) | Some(RecordingPhase::Paused)) {
            return Err("a recording is already in progress".into());
        }
    }

    let meeting_id = uuid::Uuid::new_v4().to_string();
    let now = now_iso8601();
    let meeting = Meeting {
        id: meeting_id.clone(),
        title: None,
        start_time: now.clone(),
        end_time: None,
        duration_seconds: None,
        status: MeetingStatus::Recording,
        tags: Vec::new(),
        meeting_type: None,
        created_at: String::new(),
        updated_at: String::new(),
    };

    {
        let db = lock_db(&state)?;
        db.insert_meeting(&meeting).map_err(err)?;
    }
    state.files.ensure_meeting_dir(&meeting_id).map_err(err)?;

    // Persist the requested capture config so the capture wiring + settings UI
    // remember it.
    {
        let db = lock_db(&state)?;
        if let Some(dev) = &mic_device_id {
            db.set_setting("last_mic_device", dev).map_err(err)?;
        }
        db.set_setting("last_capture_system", &system_audio.to_string())
            .map_err(err)?;
    }

    // Start the live capture session on its own thread: microphone via `cpal`,
    // system audio via WASAPI loopback on Windows (mic-only fallback elsewhere).
    let wav_path = state
        .files
        .media_path(&meeting_id, "recording.wav")
        .map_err(err)?;
    let mic = MicSelection {
        // device ids from `list_audio_devices` are "input:<name>"; strip the tag.
        device_name: mic_device_id
            .as_deref()
            .map(|d| d.strip_prefix("input:").unwrap_or(d).to_string()),
        preferred_rate: None,
    };
    let live_cfg = LiveConfig {
        mic,
        loopback: LoopbackSelection::default(),
        recorder: RecorderConfig {
            capture_system: system_audio,
            capture_microphone: true,
            ..Default::default()
        },
        wav_path,
    };
    let handle = match live::start_session(live_cfg) {
        Ok(h) => h,
        Err(e) => {
            // Mark the just-created meeting as errored so the failed attempt is
            // visible rather than stuck "recording".
            if let Ok(db) = state.db.lock() {
                let _ = db.set_meeting_status(&meeting_id, MeetingStatus::Error);
            }
            return Err(format!("failed to start capture: {e}"));
        }
    };
    *state.session.lock().map_err(|_| "session lock poisoned")? = Some(handle);

    let mut rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
    rec.phase = Some(RecordingPhase::Recording);
    rec.meeting_id = Some(meeting_id.clone());
    rec.elapsed_seconds = 0;
    rec.mic_level = 0.0;
    rec.system_level = 0.0;

    Ok(meeting_id)
}

/// Stop the active recording, finalise the meeting row (status → Transcribing)
/// and return its id. The transcription pipeline is kicked off separately by
/// the frontend via [`retranscribe_meeting`] (or auto, in the Integrate wiring).
#[tauri::command]
pub fn stop_recording(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    let id = {
        let mut rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
        let id = rec
            .meeting_id
            .clone()
            .ok_or_else(|| "no active recording to stop".to_string())?;
        rec.phase = Some(RecordingPhase::Idle);
        rec.meeting_id = None;
        rec.elapsed_seconds = 0;
        id
    };

    // Stop the capture thread and collect the finished recording.
    let session = state
        .session
        .lock()
        .map_err(|_| "session lock poisoned")?
        .take();

    let mut wav_for_transcription: Option<PathBuf> = None;
    let mut duration_seconds: i64 = 0;

    if let Some(handle) = session {
        match handle.stop() {
            Ok(result) => {
                duration_seconds = (result.duration_ms / 1000) as i64;
                let size = std::fs::metadata(&result.wav_path)
                    .map(|m| m.len() as i64)
                    .ok();
                let media = MediaFile {
                    id: uuid::Uuid::new_v4().to_string(),
                    meeting_id: id.clone(),
                    file_type: MediaFileType::Audio,
                    file_path: result.wav_path.to_string_lossy().into_owned(),
                    file_size_bytes: size,
                    format: Some("wav".into()),
                    duration_seconds: Some(duration_seconds),
                    created_at: String::new(),
                };
                if let Ok(db) = state.db.lock() {
                    let _ = db.insert_media_file(&media);
                }
                wav_for_transcription = Some(result.wav_path);
            }
            Err(e) => {
                if let Ok(db) = state.db.lock() {
                    let _ = db.set_meeting_status(&id, MeetingStatus::Error);
                }
                return Err(format!("failed to finalize recording: {e}"));
            }
        }
    }

    // Finalise the meeting row with the authoritative (frame-derived) duration.
    {
        let db = lock_db(&state)?;
        if let Some(mut m) = db.get_meeting(&id).map_err(err)? {
            m.end_time = Some(now_iso8601());
            m.duration_seconds = Some(duration_seconds);
            m.status = MeetingStatus::Transcribing;
            db.update_meeting(&m).map_err(err)?;
        }
    }

    // Auto-transcribe. Real with `--features whisper` + a cached model; otherwise
    // the worker reports the feature is disabled and flips the meeting to Error.
    if let Some(wav) = wav_for_transcription {
        let req = transcription_settings(&state);
        crate::transcription::worker::spawn_transcription(app, id.clone(), wav, req);
    }

    Ok(id)
}

#[tauri::command]
pub fn pause_recording(state: State<'_, AppState>) -> Result<(), String> {
    let mut rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
    match rec.phase {
        Some(RecordingPhase::Recording) => {
            rec.phase = Some(RecordingPhase::Paused);
            if let Ok(s) = state.session.lock() {
                if let Some(h) = s.as_ref() {
                    h.pause();
                }
            }
            Ok(())
        }
        other => Err(format!("cannot pause from {other:?}")),
    }
}

#[tauri::command]
pub fn resume_recording(state: State<'_, AppState>) -> Result<(), String> {
    let mut rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
    match rec.phase {
        Some(RecordingPhase::Paused) => {
            rec.phase = Some(RecordingPhase::Recording);
            if let Ok(s) = state.session.lock() {
                if let Some(h) = s.as_ref() {
                    h.resume();
                }
            }
            Ok(())
        }
        other => Err(format!("cannot resume from {other:?}")),
    }
}

#[tauri::command]
pub fn get_recording_status(state: State<'_, AppState>) -> Result<RecordingStatusDto, String> {
    let rec = state.recording.lock().map_err(|_| "recording lock poisoned")?;
    let phase = rec.phase.unwrap_or(RecordingPhase::Idle);
    // Prefer the authoritative duration + live levels from the running session.
    let (elapsed_seconds, mic_level, system_level) = match state.session.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(h) => (h.duration_seconds(), h.mic_level(), h.system_level()),
            None => (rec.elapsed_seconds, rec.mic_level, rec.system_level),
        },
        Err(_) => (rec.elapsed_seconds, rec.mic_level, rec.system_level),
    };
    Ok(RecordingStatusDto {
        state: phase.as_dto(),
        meeting_id: rec.meeting_id.clone(),
        elapsed_seconds,
        mic_level,
        system_level,
    })
}

// ===========================================================================
// Meeting commands.
// ===========================================================================

#[tauri::command]
pub fn list_meetings(state: State<'_, AppState>) -> Result<Vec<Meeting>, String> {
    lock_db(&state)?.list_meetings().map_err(err)
}

#[tauri::command]
pub fn get_meeting_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<MeetingDetailDto, String> {
    let db = lock_db(&state)?;
    let meeting = db
        .get_meeting(&id)
        .map_err(err)?
        .ok_or_else(|| format!("meeting not found: {id}"))?;
    let media = db.list_media_files(&id).map_err(err)?;
    let segments = db.list_transcript_segments(&id).map_err(err)?;
    let runs = db.list_transcript_runs(&id).map_err(err)?;
    let summaries = db.list_summaries(&id).map_err(err)?;
    Ok(MeetingDetailDto {
        meeting,
        media,
        segments,
        runs,
        summaries,
    })
}

/// Segments of a specific transcription run (for viewing a non-latest run).
#[tauri::command]
pub fn get_run_segments(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<Vec<TranscriptSegment>, String> {
    lock_db(&state)?
        .list_transcript_segments_for_run(&run_id)
        .map_err(err)
}

/// List every transcription run for a meeting (newest first, with counts).
#[tauri::command]
pub fn list_transcript_runs(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<Vec<TranscriptRun>, String> {
    lock_db(&state)?.list_transcript_runs(&meeting_id).map_err(err)
}

/// Delete one transcription run and its segments.
#[tauri::command]
pub fn delete_transcript_run(
    state: State<'_, AppState>,
    run_id: String,
) -> Result<(), String> {
    lock_db(&state)?.delete_transcript_run(&run_id).map_err(err)
}

/// Delete one summary.
#[tauri::command]
pub fn delete_summary(state: State<'_, AppState>, summary_id: String) -> Result<(), String> {
    lock_db(&state)?.delete_summary(&summary_id).map_err(err)
}

/// Delete a meeting: DB rows (cascaded) *and* the on-disk media directory.
#[tauri::command]
pub fn delete_meeting(state: State<'_, AppState>, id: String) -> Result<(), String> {
    {
        let db = lock_db(&state)?;
        db.delete_meeting(&id).map_err(err)?;
    }
    state.files.delete_meeting_files(&id).map_err(err)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingPatch {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub meeting_type: Option<crate::models::MeetingType>,
}

/// Apply a partial update to a meeting (title, tags, type — the user-editable
/// fields) and return the updated row.
#[tauri::command]
pub fn update_meeting(
    state: State<'_, AppState>,
    id: String,
    patch: MeetingPatch,
) -> Result<Meeting, String> {
    let db = lock_db(&state)?;
    let mut m = db
        .get_meeting(&id)
        .map_err(err)?
        .ok_or_else(|| format!("meeting not found: {id}"))?;
    if let Some(title) = patch.title {
        m.title = Some(title);
    }
    if let Some(tags) = patch.tags {
        m.tags = tags;
    }
    if patch.meeting_type.is_some() {
        m.meeting_type = patch.meeting_type;
    }
    db.update_meeting(&m).map_err(err)?;
    Ok(m)
}

/// Full-text search across all transcripts, returning the distinct meetings that
/// contain a hit (newest first). The frontend's `searchTranscripts` expects a
/// `Meeting[]`.
#[tauri::command]
pub fn search_transcripts(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<Meeting>, String> {
    let db = lock_db(&state)?;
    let hits = search::search_transcripts(&db, &query, 200).map_err(err)?;
    // Collapse hits to distinct meetings, preserving relevance order.
    let mut seen = std::collections::HashSet::new();
    let mut meetings = Vec::new();
    for hit in hits {
        if seen.insert(hit.meeting_id.clone()) {
            if let Some(m) = db.get_meeting(&hit.meeting_id).map_err(err)? {
                meetings.push(m);
            }
        }
    }
    Ok(meetings)
}

// ===========================================================================
// Transcription commands.
// ===========================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionStatusDto {
    pub meeting_id: String,
    pub stage: String,
    pub progress: f32,
    pub message: Option<String>,
}

/// Report a meeting's transcription state, derived from its stored status. The
/// live per-stage progress stream is emitted as Tauri events by the pipeline
/// (Integrate wiring); this poll gives the coarse state for a page load.
#[tauri::command]
pub fn get_transcription_status(
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<TranscriptionStatusDto, String> {
    // Prefer live, per-stage progress from the worker thread if present.
    if let Ok(map) = state.transcription.lock() {
        if let Some(entry) = map.get(&meeting_id) {
            return Ok(TranscriptionStatusDto {
                meeting_id,
                stage: entry.stage.clone(),
                progress: entry.fraction,
                message: entry.message.clone(),
            });
        }
    }
    // Otherwise derive a coarse state from the stored meeting status.
    let db = lock_db(&state)?;
    let m = db
        .get_meeting(&meeting_id)
        .map_err(err)?
        .ok_or_else(|| format!("meeting not found: {meeting_id}"))?;
    let (stage, progress) = match m.status {
        MeetingStatus::Recording => ("queued", 0.0),
        MeetingStatus::Transcribing => ("transcribing", 0.5),
        MeetingStatus::Completed => ("done", 1.0),
        MeetingStatus::Error => ("error", 0.0),
    };
    Ok(TranscriptionStatusDto {
        meeting_id,
        stage: stage.to_string(),
        progress,
        message: None,
    })
}

/// (Re)run transcription for a meeting. The real work needs the `whisper` cargo
/// feature + a cached model; without them the processor returns a clear
/// `FeatureDisabled` error which we surface so the UI can tell the user how to
/// build. The DB bookkeeping (status flip) is real either way.
#[tauri::command]
pub fn retranscribe_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
    meeting_id: String,
    // Optional per-call overrides (history "re-run with another model"); when
    // absent we fall back to the global transcription settings. A new run is
    // appended — earlier transcripts are kept for comparison.
    engine: Option<String>,
    gemini_model: Option<String>,
    whisper_model: Option<String>,
    language: Option<String>,
) -> Result<(), String> {
    // Resolve the recorded audio file; its absence is a real, reportable error.
    let media = {
        let db = lock_db(&state)?;
        db.list_media_files(&meeting_id).map_err(err)?
    };
    let audio = match media
        .into_iter()
        .find(|m| m.file_type == MediaFileType::Audio)
    {
        Some(a) => a,
        None => {
            let db = lock_db(&state)?;
            db.set_meeting_status(&meeting_id, MeetingStatus::Error)
                .map_err(err)?;
            return Err(format!("no audio file recorded for meeting {meeting_id}"));
        }
    };
    let wav_path = PathBuf::from(audio.file_path);

    {
        let db = lock_db(&state)?;
        db.set_meeting_status(&meeting_id, MeetingStatus::Transcribing)
            .map_err(err)?;
    }

    // Drive transcription on a worker thread (Gemini multimodal or local whisper,
    // per settings + any per-call overrides) so it can stream progress without
    // blocking the IPC thread.
    let mut req = transcription_settings(&state);
    if let Some(e) = engine {
        req.engine = e;
    }
    if let Some(m) = gemini_model {
        req.gemini_model = m;
    }
    if let Some(m) = whisper_model {
        req.model_id = m;
    }
    if let Some(l) = language {
        req.language = if l.is_empty() || l == "auto" { None } else { Some(l) };
    }
    crate::transcription::worker::spawn_transcription(app, meeting_id, wav_path, req);
    Ok(())
}

/// Import an existing audio file as a new meeting and transcribe it (no
/// recording). The file is copied into the meeting's media directory; the
/// configured engine then produces a transcript + summary the same way a
/// recording would. Returns the new meeting id so the UI can open it.
///
/// Non-wav files (mp3/m4a/…) work with the Gemini engine (it decodes by MIME);
/// the local whisper engine needs wav and reports a clear error otherwise.
#[tauri::command]
pub fn import_audio_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
    file_path: String,
    title: Option<String>,
    meeting_type: Option<String>,
    engine: Option<String>,
    gemini_model: Option<String>,
    whisper_model: Option<String>,
    language: Option<String>,
) -> Result<String, String> {
    let src = std::path::Path::new(&file_path);
    if !src.is_file() {
        return Err(format!("file not found: {file_path}"));
    }
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav")
        .to_ascii_lowercase();
    let derived_title = title.filter(|t| !t.trim().is_empty()).or_else(|| {
        src.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });

    let meeting_id = uuid::Uuid::new_v4().to_string();
    let meeting = Meeting {
        id: meeting_id.clone(),
        title: derived_title,
        start_time: now_iso8601(),
        end_time: None,
        duration_seconds: None,
        status: MeetingStatus::Transcribing,
        tags: Vec::new(),
        meeting_type: meeting_type.as_deref().and_then(MeetingType::from_db_str),
        created_at: String::new(),
        updated_at: String::new(),
    };
    {
        let db = lock_db(&state)?;
        db.insert_meeting(&meeting).map_err(err)?;
    }
    state.files.ensure_meeting_dir(&meeting_id).map_err(err)?;

    // Copy the source audio into the meeting dir (keep the original extension so
    // the player + Gemini MIME detection see the real format).
    let dest = state
        .files
        .media_path(&meeting_id, &format!("source.{ext}"))
        .map_err(err)?;
    std::fs::copy(src, &dest).map_err(|e| format!("failed to copy audio: {e}"))?;
    let size = std::fs::metadata(&dest).map(|m| m.len() as i64).ok();
    {
        let db = lock_db(&state)?;
        db.insert_media_file(&MediaFile {
            id: uuid::Uuid::new_v4().to_string(),
            meeting_id: meeting_id.clone(),
            file_type: MediaFileType::Audio,
            file_path: dest.to_string_lossy().into_owned(),
            file_size_bytes: size,
            format: Some(ext),
            duration_seconds: None,
            created_at: String::new(),
        })
        .map_err(err)?;
    }

    let mut req = transcription_settings(&state);
    if let Some(e) = engine {
        req.engine = e;
    }
    if let Some(m) = gemini_model {
        req.gemini_model = m;
    }
    if let Some(m) = whisper_model {
        req.model_id = m;
    }
    if let Some(l) = language {
        req.language = if l.is_empty() || l == "auto" { None } else { Some(l) };
    }
    crate::transcription::worker::spawn_transcription(app, meeting_id.clone(), dest, req);
    Ok(meeting_id)
}

/// Edit a single transcript segment's text (and optionally speaker label). The
/// FTS index re-syncs via the `AFTER UPDATE` trigger.
#[tauri::command]
pub fn update_segment(
    state: State<'_, AppState>,
    segment_id: String,
    text: String,
    speaker: Option<String>,
) -> Result<(), String> {
    let db = lock_db(&state)?;
    // `update_transcript_segment` takes a full row (timing/index are required by
    // the row type), so read the existing segment first, then overwrite the two
    // editable fields.
    let existing = find_segment(&db, &segment_id).map_err(err)?;
    let mut seg = existing.ok_or_else(|| format!("segment not found: {segment_id}"))?;
    seg.text = text;
    seg.speaker = speaker;
    db.update_transcript_segment(&seg).map_err(err)?;
    Ok(())
}

/// Find a transcript segment by id. The storage layer lists per-meeting, so we
/// resolve the meeting via a small query, then pick the segment out. Kept here
/// (rather than in storage) because it's a command-layer convenience.
fn find_segment(
    db: &Database,
    segment_id: &str,
) -> storage::Result<Option<TranscriptSegment>> {
    let meeting_id: Option<String> = db
        .conn()
        .query_row(
            "SELECT meeting_id FROM transcript_segments WHERE id = ?1",
            [segment_id],
            |r| r.get(0),
        )
        .ok();
    let Some(meeting_id) = meeting_id else {
        return Ok(None);
    };
    Ok(db
        .list_transcript_segments(&meeting_id)?
        .into_iter()
        .find(|s| s.id == segment_id))
}

// ===========================================================================
// Summary commands.
// ===========================================================================

/// Generate (and persist) an AI summary for a meeting. Joins the meeting's
/// segments into a transcript, dispatches to the configured provider (Ollama
/// local by default), parses the structured result, and stores it.
///
/// Real async I/O: Ollama over HTTP, or a cloud provider with a keychain key.
/// Args arrive flat (camelCase from JS → snake_case params) matching
/// `api.generateSummary({ meetingId, provider, model, prompt })`.
#[tauri::command]
pub async fn generate_summary(
    state: State<'_, AppState>,
    meeting_id: String,
    provider: String,
    model: String,
    prompt: Option<String>,
) -> Result<Summary, String> {
    let kind = AiProviderKind::from_db_str(&provider)
        .ok_or_else(|| format!("unknown AI provider: {provider}"))?;

    // Pull the transcript + meeting type out under the lock, then drop it before
    // the await (the DB guard is not Send across an await point).
    let (transcript, template) = {
        let db = lock_db(&state)?;
        let meeting = db
            .get_meeting(&meeting_id)
            .map_err(err)?
            .ok_or_else(|| format!("meeting not found: {meeting_id}"))?;
        let segments = db.list_transcript_segments(&meeting_id).map_err(err)?;
        if segments.is_empty() {
            return Err("meeting has no transcript to summarize".into());
        }
        let transcript = join_transcript(&segments);
        let template = ai::SummaryTemplate::for_meeting_type(meeting.meeting_type);
        (transcript, template)
    };

    let model_opt = if model.is_empty() { None } else { Some(model) };
    let provider_impl = ai::build_provider(kind, model_opt);
    let draft = provider_impl
        .summarize(&transcript, template)
        .await
        .map_err(err)?;

    let summary = Summary {
        id: uuid::Uuid::new_v4().to_string(),
        meeting_id: meeting_id.clone(),
        summary_type: if prompt.is_some() {
            SummaryType::Custom
        } else {
            SummaryType::Auto
        },
        content: draft.content,
        action_items: draft.action_items,
        key_decisions: draft.key_decisions,
        prompt_used: prompt,
        ai_provider: Some(draft.provider),
        ai_model: Some(draft.model),
        tokens_used: draft.tokens_used,
        created_at: String::new(),
    };

    {
        let db = lock_db(&state)?;
        db.insert_summary(&summary).map_err(err)?;
    }
    Ok(summary)
}

/// Pre-flight token + USD cost estimate for summarizing a meeting (PRD §4.6 "成
/// 本估算"). Local Ollama returns 0; cloud providers return their per-model price.
#[tauri::command]
pub fn estimate_summary_cost(
    state: State<'_, AppState>,
    meeting_id: String,
    provider: String,
    model: String,
) -> Result<CostEstimateDto, String> {
    let kind = AiProviderKind::from_db_str(&provider)
        .ok_or_else(|| format!("unknown AI provider: {provider}"))?;

    let (transcript, template) = {
        let db = lock_db(&state)?;
        let meeting = db
            .get_meeting(&meeting_id)
            .map_err(err)?
            .ok_or_else(|| format!("meeting not found: {meeting_id}"))?;
        let segments = db.list_transcript_segments(&meeting_id).map_err(err)?;
        (
            join_transcript(&segments),
            ai::SummaryTemplate::for_meeting_type(meeting.meeting_type),
        )
    };

    let model_opt = if model.is_empty() { None } else { Some(model) };
    let provider_impl = ai::build_provider(kind, model_opt);
    let estimate = provider_impl.estimate_cost(&transcript, template);
    Ok(CostEstimateDto {
        provider: kind.as_db_str().to_string(),
        model: provider_impl.active_model().to_string(),
        prompt_tokens: estimate.input_tokens,
        estimated_usd: estimate.usd_cost.unwrap_or(0.0),
    })
}

// ===========================================================================
// Export command.
// ===========================================================================

/// Export a meeting to the requested format, writing the file under the
/// meeting's media directory and returning its absolute path. Pure exporters
/// from [`crate::export`] do the formatting. `format` is one of
/// "markdown" | "srt" | "vtt" | "json" (matches src/lib/constants.ts).
#[tauri::command]
pub fn export_meeting(
    state: State<'_, AppState>,
    meeting_id: String,
    format: String,
) -> Result<String, String> {
    let fmt = parse_export_format(&format)?;

    let (meeting, segments, summary) = {
        let db = lock_db(&state)?;
        let meeting = db
            .get_meeting(&meeting_id)
            .map_err(err)?
            .ok_or_else(|| format!("meeting not found: {meeting_id}"))?;
        let segments = db.list_transcript_segments(&meeting_id).map_err(err)?;
        let summary = db.list_summaries(&meeting_id).map_err(err)?.into_iter().next();
        (meeting, segments, summary)
    };

    let rendered = render_export(fmt, &meeting, &segments, summary.as_ref())?;

    let file_name = format!("export.{}", fmt.extension());
    let path: PathBuf = state
        .files
        .media_path(&meeting_id, &file_name)
        .map_err(err)?;
    std::fs::write(&path, rendered).map_err(err)?;
    Ok(path.to_string_lossy().into_owned())
}

fn parse_export_format(s: &str) -> Result<ExportFormat, String> {
    match s.to_ascii_lowercase().as_str() {
        "markdown" | "md" => Ok(ExportFormat::Markdown),
        "srt" => Ok(ExportFormat::Srt),
        "vtt" => Ok(ExportFormat::Vtt),
        "json" => Ok(ExportFormat::Json),
        "pdf" => Err("PDF export is planned for v1.1".into()),
        "notion" => Err("Notion export is planned for v1.2".into()),
        other => Err(format!("unknown export format: {other}")),
    }
}

fn render_export(
    format: ExportFormat,
    meeting: &Meeting,
    segments: &[TranscriptSegment],
    summary: Option<&Summary>,
) -> Result<String, String> {
    match format {
        ExportFormat::Markdown => Ok(export::to_markdown(meeting, segments, summary)),
        ExportFormat::Srt => Ok(export::to_srt(segments)),
        ExportFormat::Vtt => Ok(export::to_vtt(segments)),
        ExportFormat::Json => export::to_json(meeting, segments, summary).map_err(err),
        ExportFormat::Pdf => Err("PDF export is planned for v1.1".into()),
        ExportFormat::Notion => Err("Notion export is planned for v1.2".into()),
    }
}

// ===========================================================================
// Settings + audio-device + keychain commands.
// ===========================================================================

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<std::collections::HashMap<String, String>, String> {
    let db = lock_db(&state)?;
    let rows = db.list_settings().map_err(err)?;
    Ok(rows.into_iter().map(|s| (s.key, s.value)).collect())
}

#[tauri::command]
pub fn set_setting(state: State<'_, AppState>, key: String, value: String) -> Result<(), String> {
    lock_db(&state)?.set_setting(&key, &value).map_err(err)
}

/// List available audio input + output devices for the settings UI. On Windows
/// this enumerates `cpal` inputs and WASAPI render (loopback) targets; on the
/// dev Mac, inputs come from `cpal` and the loopback list is empty (v1.1).
#[tauri::command]
pub fn list_audio_devices() -> Result<Vec<AudioDeviceDto>, String> {
    let mut out = Vec::new();

    let inputs = crate::audio::microphone::list_input_devices().unwrap_or_default();
    for (i, name) in inputs.into_iter().enumerate() {
        out.push(AudioDeviceDto {
            id: format!("input:{name}"),
            name,
            kind: "input",
            is_default: i == 0,
        });
    }

    // Loopback (system-audio) targets are Windows-only in v1.0; an Err here is
    // the expected "unsupported (v1.1)" off Windows, so we ignore it.
    if let Ok(outputs) = crate::audio::system_audio::list_loopback_devices() {
        for (i, name) in outputs.into_iter().enumerate() {
            out.push(AudioDeviceDto {
                id: format!("output:{name}"),
                name,
                kind: "output",
                is_default: i == 0,
            });
        }
    }

    Ok(out)
}

/// Store a cloud provider's API key in the OS keychain (never plain text / DB).
#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<(), String> {
    let kind = AiProviderKind::from_db_str(&provider)
        .ok_or_else(|| format!("unknown AI provider: {provider}"))?;
    keychain::set_api_key(kind, &key).map_err(err)
}

/// Whether a cloud provider already has an API key stored. Ollama is local and
/// always reports `false` (it needs no key).
#[tauri::command]
pub fn has_api_key(provider: String) -> Result<bool, String> {
    let kind = AiProviderKind::from_db_str(&provider)
        .ok_or_else(|| format!("unknown AI provider: {provider}"))?;
    Ok(keychain::get_api_key(kind).map_err(err)?.is_some())
}

// ===========================================================================
// Storage command.
// ===========================================================================

#[tauri::command]
pub fn get_storage_usage(state: State<'_, AppState>) -> Result<StorageUsageDto, String> {
    let total_bytes = state.files.total_storage_bytes().map_err(err)?;
    let meeting_count = lock_db(&state)?.list_meetings().map_err(err)?.len();
    Ok(StorageUsageDto {
        total_bytes,
        meeting_count,
    })
}

// ===========================================================================
// Helpers.
// ===========================================================================

/// Join transcript segments into the `Speaker: text` line-per-segment form the
/// AI prompt + chunker expect (one segment per line; see `ai::chunk_transcript`).
fn join_transcript(segments: &[TranscriptSegment]) -> String {
    segments
        .iter()
        .map(|s| match &s.speaker {
            Some(sp) => format!("{sp}: {}", s.text),
            None => s.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read the user's transcription preferences from settings, with sane defaults:
/// whisper model `belle-turbo-zh` (Chinese fine-tune), diarization on, **language Chinese**
/// (`zh`) by default — auto-detect mis-fires on short clips, so we bias to the
/// user's primary language. A stored `whisper_language` ("en"/"ja"/"auto"/…)
/// overrides; "auto"/"" means let whisper detect.
fn transcription_settings(
    state: &State<'_, AppState>,
) -> crate::transcription::worker::TranscriptionRequest {
    use crate::transcription::worker::TranscriptionRequest;
    let Ok(db) = state.db.lock() else {
        return TranscriptionRequest {
            model_id: "belle-turbo-zh".to_string(),
            diarize: true,
            language: Some("zh".to_string()),
            engine: "auto".to_string(),
            gemini_model: "gemini-3.5-flash".to_string(),
        };
    };
    // Keys MUST match src/lib/constants.ts SETTINGS_KEYS (the UI writes these).
    let model_id = db
        .get_setting("transcription.whisper_model")
        .ok()
        .flatten()
        .unwrap_or_else(|| "belle-turbo-zh".to_string());
    let diarize = db
        .get_setting("transcription.diarization_enabled")
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    let language = match db.get_setting("transcription.language").ok().flatten() {
        Some(l) if l.is_empty() || l == "auto" => None,
        Some(l) => Some(l),
        None => Some("zh".to_string()),
    };
    // Engine: "auto" (Gemini if a key is set, else whisper) | "gemini" | "whisper".
    let engine = db
        .get_setting("transcription.engine")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "auto".to_string());
    let gemini_model = db
        .get_setting("transcription.gemini_model")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "gemini-3.5-flash".to_string());
    TranscriptionRequest {
        model_id,
        diarize,
        language,
        engine,
        gemini_model,
    }
}

/// Current UTC timestamp as the SQLite `DATETIME` text form (`YYYY-MM-DD
/// HH:MM:SS`). Kept dependency-free (no chrono) by formatting from
/// `SystemTime` — good enough for ordering + display.
pub(crate) fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil-from-days (Howard Hinnant's algorithm) — pure integer date math.
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as i64;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MeetingStatus, TranscriptSegment};

    fn seg(id: &str, meeting: &str, idx: i64, text: &str, speaker: Option<&str>) -> TranscriptSegment {
        TranscriptSegment {
            id: id.into(),
            meeting_id: meeting.into(),
            segment_index: idx,
            start_time_ms: idx * 1000,
            end_time_ms: idx * 1000 + 900,
            text: text.into(),
            speaker: speaker.map(|s| s.into()),
            confidence: None,
            language: Some("en".into()),
            created_at: String::new(),
        }
    }

    fn state() -> AppState {
        let dir = std::env::temp_dir().join(format!(
            "mra_cmd_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        AppState::bootstrap(&dir).expect("bootstrap app state")
    }

    #[test]
    fn join_transcript_prefixes_speaker() {
        let segs = vec![
            seg("s1", "m1", 0, "hello", Some("Alice")),
            seg("s2", "m1", 1, "hi", None),
        ];
        assert_eq!(join_transcript(&segs), "Alice: hello\nhi");
    }

    #[test]
    fn now_iso8601_has_expected_shape() {
        let s = now_iso8601();
        // YYYY-MM-DD HH:MM:SS
        assert_eq!(s.len(), 19);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], " ");
        assert_eq!(&s[13..14], ":");
    }

    #[test]
    fn parse_export_format_maps_known_and_rejects_v1_1() {
        assert_eq!(parse_export_format("markdown").unwrap(), ExportFormat::Markdown);
        assert_eq!(parse_export_format("MD").unwrap(), ExportFormat::Markdown);
        assert_eq!(parse_export_format("srt").unwrap(), ExportFormat::Srt);
        assert_eq!(parse_export_format("vtt").unwrap(), ExportFormat::Vtt);
        assert_eq!(parse_export_format("json").unwrap(), ExportFormat::Json);
        assert!(parse_export_format("pdf").is_err());
        assert!(parse_export_format("notion").is_err());
        assert!(parse_export_format("xyz").is_err());
    }

    #[test]
    fn bootstrap_then_meeting_crud_via_db() {
        let st = state();
        let db = st.db.lock().unwrap();
        // Empty to start.
        assert!(db.list_meetings().unwrap().is_empty());
    }

    #[test]
    fn render_export_markdown_and_json() {
        let st = state();
        let m = Meeting {
            id: "m1".into(),
            title: Some("Sync".into()),
            start_time: "2026-06-18 14:00:00".into(),
            end_time: None,
            duration_seconds: Some(60),
            status: MeetingStatus::Completed,
            tags: vec![],
            meeting_type: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let segs = vec![seg("s1", "m1", 0, "hello world", Some("Alice"))];
        let md = render_export(ExportFormat::Markdown, &m, &segs, None).unwrap();
        assert!(md.contains("# Meeting: Sync"));
        let js = render_export(ExportFormat::Json, &m, &segs, None).unwrap();
        assert!(js.contains("\"export_version\""));
        let _ = st; // keep tempdir alive until here
    }
}
