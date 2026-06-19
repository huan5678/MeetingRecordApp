# Windows Handoff — taking v1.0 from "compiles" to "records"

> **Why this doc:** v1.0 is **Windows-first** but the scaffold was built/verified on macOS, where the
> system-audio path can't run. Everything below is the work that must happen **on a Windows machine
> (or a VM with audio passthrough)** to reach a working recorder. See `docs/PRD.md` §4.4/§7.3/§7.5.

## Current state (verified on macOS, 2026-06-19)
- ✅ Frontend: `npm run build` clean, 40 vitest tests pass.
- ✅ Backend **default features**: `cargo check` clean, `cargo test` = **227 passed / 0 failed**.
- ⏳ `--features whisper,diarize`: **not yet built** (needs native deps + models).
- ❌ Live audio capture (Phase 2) and the whisper/diarize worker pipeline (Phase 3) are **stubbed**.

---

## 1. Prerequisites on Windows
| Tool | Why | Notes |
|---|---|---|
| **Rust** (rustup, stable) | backend | `winget install Rustlang.Rustup` then `rustup default stable-msvc` |
| **VS Build Tools (MSVC + Windows SDK)** | links Rust + native libs | "Desktop development with C++" workload |
| **WebView2 Runtime** | Tauri renders the UI | preinstalled on Win11; else install Evergreen runtime |
| **Node ≥ 20** | frontend | `winget install OpenJS.NodeJS.LTS` |
| **CMake** | builds whisper.cpp (whisper-rs) | only needed for `--features whisper` |
| **Git** | repo | |

## 2. Get it running (audio-only, no native features)
```powershell
npm install
npm run tauri dev        # debug run; or `npm run tauri build` for an installer
```
This should launch the app with the mock-data UI and the real command layer. The DB is created
in the app-data dir; recordings go under it (see `storage::FileStore`).

## 3. Phase 0 Spike — the #1 risk, do this FIRST
Per PRD §7.3, before building more, **prove the audio core works on Windows**:
- Capture **system audio** (WASAPI loopback, `src-tauri/src/audio/system_audio.rs`, `#[cfg(windows)]`)
  **+ mic** (`microphone.rs`, cpal) simultaneously.
- Run them through `mixer.rs` (handles sample-rate / clock **drift** — already unit-tested) → 16 kHz mono.
- Write a WAV via `recorder.rs` (`hound`).
- **Success = play back a 30 s clip and hear BOTH the system audio and the mic, no drift/desync.**

The pure mixer math is tested; what's unverified is the *live device threads*. The functions exist as
clean stubs/structure — wire the capture callbacks to feed `Recorder`.

## 4. Remaining implementation (Phase 2 & 3)
- **Phase 2 — live capture wiring:** `start_recording` currently creates the meeting row + dir + an
  in-memory FSM but does **not** spawn the cpal/WASAPI threads or the `Recorder::feed` loop / level
  metering / optional dual-track. Wire those.
- **Phase 3 — transcription worker:** `retranscribe_meeting` flips status + checks the file but does
  **not** run the pipeline. Build with the features below, then run whisper→diarize on a **spawned
  thread** emitting Tauri `Progress` events (`processor.rs` already exposes an `on_progress` hook).

## 5. Enable native features (transcription + diarization)
```powershell
cargo check --features whisper,diarize          # from src-tauri/
```
- **whisper-rs** compiles whisper.cpp (needs CMake). Default model `small`; download a GGUF/GGML
  whisper model into the model cache dir (`transcription::model`), or wire download-on-first-use.
- **sherpa-onnx** needs its lib + the **segmentation** and **speaker-embedding** ONNX models.
- For GPU: try CUDA / Vulkan / DirectML feature flags of whisper-rs as available.

## 6. AI summary
- **Local (default):** run `ollama serve` (the app talks to `http://127.0.0.1:11434`). `ollama pull`
  a model (e.g. `llama3`) and select it in Settings.
- **Cloud (optional):** store keys via the `set_api_key` command (OS keychain) — never plain text.

## 7. Known cleanup items (not blockers)
- **Scope creep:** `src-tauri/src/export/notion.rs` and `export/pdf.rs` are beyond v1.0 (PRD: PDF=v1.1,
  Notion=v1.2). Delete or leave as stubs.
- **`npm audit`:** 5 vulns (1 critical) in transitive deps — triage before release.
- **3 cargo warnings:** unused imports / dead fields in `transcription/processor.rs` — only live under
  the `whisper`/`diarize` features; gate the imports under `#[cfg(feature=…)]` when you wire Phase 3.
- **`src-tauri/icons/`** currently holds a generated **placeholder** icon set (indigo square) + unused
  android/ios variants — replace with the real logo via `npm run tauri icon <logo.png>`.

## 8. Verify-after-changes checklist
```powershell
# frontend
npm run build ; npm test
# backend
cd src-tauri ; cargo test                       # default features
cargo check --features whisper,diarize          # native paths
cargo test --target x86_64-pc-windows-msvc      # exercises the #[cfg(windows)] code
```
