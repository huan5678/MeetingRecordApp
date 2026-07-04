# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

MeetingRecordApp — a Tauri 2 desktop meeting recorder. React/TypeScript/Tailwind frontend, Rust backend. Captures system audio + mic, transcribes, and summarizes. **v1.0 scope: Windows-only, audio-only** (see `docs/PRD.md`, `docs/STRUCTURE.md`). macOS/Linux and screen recording are v1.1+.

## Commands

Run frontend commands from the repo root, Rust commands from `src-tauri/`.

```sh
# Frontend
npm run dev            # Vite dev server only (UI runs against mock data, no backend)
npm run tauri dev      # full app: Tauri window + Rust backend (needs the Rust toolchain)
npm run build          # tsc --noEmit && vite build  (also the type-check gate)
npm test               # vitest run (all frontend tests)
npx vitest run src/lib/format.test.ts        # one test file
npx vitest run -t "treats a zone-less"       # one test by name
npm run tauri build    # produce Windows installers

# Backend (cd src-tauri)
cargo build            # DEFAULT build: no native ML libs (compiles on a fresh Mac)
cargo test             # all backend tests
cargo test --lib ai::gemini_audio            # one module
cargo test needs_chunking                    # one test by name
cargo build --features whisper,opencc        # opt in to native pieces (see Feature gating)
```

## Feature gating (read before touching transcription/diarization)

The default Cargo build deliberately compiles **without** the heavy native ML libraries so `cargo build`/`cargo test` work on any machine:

- `whisper` → local whisper.cpp transcription (`whisper-rs`; needs **CMake**, so it does NOT build on a stock Mac)
- `diarize` → speaker diarization (`sherpa-rs`)
- `opencc` → Simplified→Traditional Chinese conversion
- `full` = all three

Code behind these is `#[cfg(feature = "…")]`; when a feature is off, the path returns a clear "feature disabled" error rather than failing to compile. **Consequence:** the Gemini transcription path needs no native deps and is the only path testable in the default build (and cross-platform). When editing whisper-feature code you cannot `cargo check` it on a Mac without CMake — validate by logic + a Windows build.

## Architecture

**Frontend ↔ backend contract.** The UI calls the backend *only* through `src/lib/tauri.ts` (`api.*` → `call()` → Tauri `invoke`). Three things must stay in lock-step or IPC silently breaks:
- Command names in `tauri.ts` `COMMANDS` ↔ the `#[tauri::command]` fns registered in `src-tauri/src/lib.rs`'s `generate_handler!`.
- `src/lib/types.ts` mirrors `src-tauri/src/models.rs` (serde uses the Rust field names verbatim — **snake_case**, no `rename_all`).
- `SETTINGS_KEYS` in `src/lib/constants.ts` ↔ the bare strings the backend reads (e.g. `commands::transcription_settings`).
- Tauri auto-converts JS camelCase command args → Rust snake_case params (`meetingId` → `meeting_id`).

**Mock mode.** When not running inside Tauri (browser dev / Vitest), `call()` falls back to `mockInvoke` + `src/lib/mocks.ts`, so the whole UI is usable without the Rust backend. Any new command needs a `mockInvoke` case and (if it feeds a view) mock data, or tests break.

**Backend state.** `commands::AppState` (managed Tauri state, mutex-guarded) holds: the SQLite `Database` (`rusqlite` + FTS5), the on-disk media `FileStore`, the live recording `session`, and a `transcription` progress map. `lib.rs::run()` bootstraps it and builds the tray.

**Transcription pipeline** (`src-tauri/src/transcription/worker.rs`). Runs on a `std::thread` (no ambient async runtime → it spins a throwaway current-thread tokio runtime and `block_on`s the async work). A `TranscriptionRequest` selects the engine: `gemini` | `whisper` | `auto` (Gemini when a key is set, else whisper). The Gemini path (`ai/gemini_audio.rs`) uploads audio via the Files API (resumable upload) and calls `generateContent`; **long WAV recordings are split into ~10-min chunks** (output-token ceiling), transcribed per-chunk with offset timestamps, then summarized via the map-reduce summarizer. Progress is pushed through `report()` → the state map + `transcription://progress|done|error` events; the frontend polls `get_transcription_status` (it only polls while non-terminal — re-running a settled meeting needs the `restartKey` mechanism in `useTranscription`).

**Versioned transcripts/summaries.** A meeting can be transcribed more than once; every result is kept. `transcript_runs` (a table) groups segments via `transcript_segments.run_id`; `summaries` is already multi-row. `get_meeting_detail` returns `runs[]` + `summaries[]`; `list_transcript_segments` returns the latest run (legacy meetings with no run fall back to all segments).

**AI providers** (`ai/provider.rs` `AiProvider` trait). Ollama is the local default; OpenAI/Claude/Gemini are cloud. `summarize()` map-reduces long transcripts. API keys live in the OS keychain (`ai/keychain.rs`) — the `keyring` crate needs its platform-native features enabled or it silently uses a non-persisting mock store.

**Storage & migrations.** Schema is `src-tauri/migrations/*.sql`, applied in `database.rs::run_migrations` via `execute_batch` (all `CREATE … IF NOT EXISTS`, so idempotent) plus Rust `PRAGMA table_info` guards for additive `ALTER`s — there is **no version table**, so new schema must be additive and idempotent. FTS5 (`transcript_fts`) is kept in sync by triggers.

**Audio** (`audio/`): `cpal` microphone + WASAPI loopback for system audio (Windows-only; mic-only fallback elsewhere), mixed to 16 kHz mono WAV.

## Conventions & gotchas

- **Platform:** v1.0 is Windows-only; non-Windows code paths return `unsupported_platform(...)` errors but still compile. Development happens on macOS, but real runtime testing happens on Windows.
- **Deploy/test loop:** `origin` is a **bare repo on the user's Windows machine**; it builds by `git fetch origin; git reset --hard origin/main`. So commits go directly to **`main`** (do not branch for this repo) and are only testable once pushed.
- **Timestamps:** the backend stores UTC wall-clock with no zone marker; the frontend (`formatDateTime`) treats zone-less strings as UTC and renders local time. Don't "fix" by parsing as local.
- `index.html` must NOT contain a CSP `<meta>` tag — it overrides `tauri.conf.json` and breaks media playback / `connect-src`.

## Agent skills

### Issue tracker

Issues and PRDs live as markdown files under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical triage roles, default strings (needs-triage, needs-info, ready-for-agent, ready-for-human, wontfix). See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.
