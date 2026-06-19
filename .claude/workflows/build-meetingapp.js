export const meta = {
  name: 'build-meetingapp',
  description: 'Scaffold + implement the MeetingRecordApp v1.0 foundation (Tauri2/Rust + React/TS) via multi-agent orchestration, per docs/PRD.md (Windows-first, audio-only, Ollama-default).',
  whenToUse: 'Drive the MeetingRecordApp build forward one pass at a time: scaffolds a buildable skeleton, implements each backend/frontend module with tests, reviews each, then integrates + verifies.',
  phases: [
    { title: 'Scaffold', detail: 'one agent builds the buildable contract: configs, Cargo.toml, migration, lib.rs/models.rs, frontend entry' },
    { title: 'Implement', detail: 'one agent per module (disjoint dirs), TDD where the toolchain allows' },
    { title: 'Review', detail: 'adversarial correctness review per module' },
    { title: 'Integrate', detail: 'wire Tauri commands/tray + verify (npm build real; cargo if present) + completeness report' },
  ],
}

// ---- Shared context handed to every agent ----
const DECISIONS = `
PROJECT: MeetingRecordApp — local-first cross-platform meeting recorder. Greenfield (only docs/ exists).
AUTHORITATIVE SPEC: read docs/PRD.md and docs/STRUCTURE.md FIRST. These v1.0 decisions are FINAL:
- v1.0 targets WINDOWS ONLY (macOS/Linux are v1.1+). Use #[cfg(target_os="windows")] for Windows-only code; keep the crate compiling on macOS via cfg + stubs that return a clear "unsupported on this platform (v1.1)" error.
- v1.0 is AUDIO ONLY. Screen recording / FFmpeg / the video module is v1.1 — leave video as a stub module with a TODO, do NOT implement it.
- App: Tauri 2.0 + React + TypeScript + Tailwind CSS. Backend: Rust.
- Audio: microphone via 'cpal'; system audio via WASAPI loopback ('wasapi' crate, cfg windows). Mixer must handle sample-rate / clock drift (resample to a common rate + buffer under/overrun); transcription path = 16kHz mono mix.
- Transcription: whisper.cpp via the 'whisper-rs' crate (NOT hand-rolled bindgen). Feature-gate it behind a cargo feature 'whisper' so the crate builds without the native lib. Default model small/base (medium/large-v3 optional).
- Diarization (v1.0, basic): sherpa-onnx (segmentation + speaker embedding). Feature-gate behind a cargo feature 'diarize'. Writes speaker label into transcript_segments.
- AI summary: provider trait with Ollama (LOCAL, DEFAULT, no api key) + OpenAI + Claude + Gemini (cloud, optional). Use 'reqwest'. Handle long transcripts via chunking + map-reduce. Provide a token/cost estimate hook for cloud providers.
- Storage: SQLite via 'rusqlite' (bundled feature) + FTS5 with the REQUIRED external-content sync triggers (see migration in PRD §4.3). 'serde'/'serde_json' for JSON columns.
- Export: Markdown, SRT/VTT, JSON (PDF optional/v1.1). Pure functions, snapshot-friendly tests.
- API keys go in the OS keychain (use 'keyring' crate); never plain text.
TOOLCHAIN REALITY (verified on the dev machine): Node/npm/pnpm present; git present; ollama present; **Rust/cargo is NOT installed**. So: WRITE idiomatic, well-typed Rust with #[cfg(test)] unit tests, but you will NOT be able to 'cargo build/test' here — say so plainly, never claim Rust compiles/passes. The frontend (npm) CAN and MUST be built/verified.
HARD RULES: surgical, idiomatic code; no placeholder lorem; real logic where feasible, explicit unimplemented!()/TODO only for hardware/native-bound paths. Do NOT invent features beyond the PRD. Match docs/STRUCTURE.md file tree exactly.
`

const IMPL_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['module', 'files_written', 'public_api', 'tests', 'unverified', 'notes'],
  properties: {
    module: { type: 'string' },
    files_written: { type: 'array', items: { type: 'string' }, description: 'repo-relative paths created/edited' },
    public_api: { type: 'array', items: { type: 'string' }, description: 'key public fns/types other modules depend on, as signatures' },
    tests: { type: 'string', description: 'what tests were written and whether they were actually run (and result)' },
    unverified: { type: 'array', items: { type: 'string' }, description: 'parts that could NOT be verified here (no toolchain / needs Windows / needs api key / needs models)' },
    notes: { type: 'string' },
  },
}

const REVIEW_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['module', 'compiles_likely', 'issues', 'verdict'],
  properties: {
    module: { type: 'string' },
    compiles_likely: { type: 'boolean', description: 'best-guess whether this module would compile, given no live compiler' },
    issues: {
      type: 'array',
      items: {
        type: 'object',
        additionalProperties: false,
        required: ['severity', 'file', 'desc'],
        properties: {
          severity: { type: 'string', enum: ['blocker', 'major', 'minor'] },
          file: { type: 'string' },
          desc: { type: 'string' },
        },
      },
    },
    verdict: { type: 'string', enum: ['ok', 'needs_fix'] },
  },
}

const MODULES = [
  {
    key: 'storage', dir: 'src-tauri/src/storage/',
    spec: `Implement the storage module per docs/STRUCTURE.md: mod.rs, database.rs (rusqlite connection, run migrations/001_initial.sql, CRUD for meetings/media_files/transcript_segments/summaries/settings), files.rs (recording dir management, storage-used calc, delete), search.rs (FTS5 keyword search over transcript_fts). Domain types come from crate::models (do NOT redefine them). Add #[cfg(test)] tests using an in-memory sqlite DB (rusqlite "bundled") covering: migration applies, insert+get meeting, insert segment -> FTS row exists (triggers), search returns hits, delete cascades.`,
  },
  {
    key: 'export', dir: 'src-tauri/src/export/',
    spec: `Implement export module: mod.rs, markdown.rs, srt.rs (SRT + VTT), json.rs. Pure functions taking crate::models types (Meeting, Vec<TranscriptSegment>, Summary) -> String. Follow the exact formats in PRD §4.8 (speaker labels included). Add snapshot-style #[cfg(test)] tests with small fixed inputs asserting exact output. PDF is out of scope here (v1.1) — leave a stub fn with TODO.`,
  },
  {
    key: 'ai', dir: 'src-tauri/src/ai/',
    spec: `Implement ai module: mod.rs, provider.rs (async trait AiProvider { summarize, name, models } + a SummaryTemplate enum for the 5 templates in PRD §4.6 + a build_prompt helper + chunking/map-reduce for long transcripts + a estimate_cost hook), ollama.rs (LOCAL default, http to /api/generate, no key), openai.rs, claude.rs, gemini.rs (cloud via reqwest; key from keyring). Parse a structured summary (content markdown + action_items + key_decisions). Add #[cfg(test)] tests for: prompt building, chunking splits a long transcript correctly, template selection, response parsing (feed a canned JSON/markdown string — do NOT hit the network in tests).`,
  },
  {
    key: 'audio', dir: 'src-tauri/src/audio/',
    spec: `Implement audio module: mod.rs, microphone.rs (cpal input capture), system_audio.rs (WASAPI loopback behind #[cfg(target_os="windows")] using 'wasapi'; on non-windows compile a stub returning an "unsupported (v1.1)" error), mixer.rs (combine mic+system: resample to a common rate, handle sample-rate/clock drift + buffer under/overrun, produce 16kHz mono for transcription AND optional dual-track) , recorder.rs (orchestrator: start/stop/pause/resume, writes WAV via 'hound', tracks duration). The mixer's resampling/mix math is PURE and MUST have thorough #[cfg(test)] tests (e.g. resample ratio, drift compensation, mono downmix, pause produces no gap corruption). Capture paths that need real devices: structure them cleanly, gate hardware calls, note they need Windows to verify.`,
  },
  {
    key: 'transcription', dir: 'src-tauri/src/transcription/',
    spec: `Implement transcription module: mod.rs, whisper.rs (whisper-rs integration behind cargo feature 'whisper'; without the feature, a stub returning a clear error), model.rs (model registry tiny/base/small/medium/large-v3, download-on-first-use with progress callback + cache dir, default small), diarization.rs (sherpa-onnx segmentation+embedding behind feature 'diarize'; stub otherwise; maps speaker labels onto whisper segments), processor.rs (pipeline: wav -> whisper segments -> diarization -> Vec<TranscriptSegment> with speaker; emits progress events). Test the PURE parts: model registry lookup/default, segment<->speaker alignment logic (with fake segments), progress accounting. FFI calls are feature-gated and noted as unverifiable here.`,
  },
  {
    key: 'detection', dir: 'src-tauri/src/detection/',
    spec: `Implement detection module: mod.rs, monitor.rs. v1.0 Windows only: poll foreground window (GetForegroundWindow + GetWindowText via 'windows' crate, #[cfg(target_os="windows")]) and match against a configurable list of meeting-app title patterns (Teams/Meet/Zoom). On non-windows, stub returning None. The MATCHING logic (title/pattern -> known app) is pure and MUST be unit-tested. Use a sensible poll interval; don't busy-loop.`,
  },
  {
    key: 'frontend', dir: 'src/',
    spec: `Implement the React/TS/Tailwind frontend skeleton EXACTLY per docs/STRUCTURE.md src/ tree: components/{Tray,Floating,Meeting,Summary,Export,Settings,common}, hooks/{useRecording,useTranscription,useMeetings}, stores/{recordingStore,settingsStore} (zustand), lib/{tauri.ts (typed @tauri-apps/api invoke wrappers — define the command names matching the Rust side), constants.ts}, styles/globals.css (Tailwind). Components should render real layout (recording controls, meeting list, detail with transcript+summary, settings with audio/AI/general tabs incl. Ollama-default + cloud-optional + whisper model select), wired to the store with MOCK data so the app runs without the backend. Dark mode support. This MUST build: ensure 'npm install' deps are declared and 'npm run build' (vite + tsc) passes — actually run it and report the result. Do NOT touch src-tauri/.`,
  },
]

// ============================================================
phase('Scaffold')
log('🏗️  Phase 1/4 — 建立可建置骨架(configs / Cargo.toml / migration / lib.rs / models.rs / 前端入口)')

const scaffold = await agent(
  `${DECISIONS}

You are the SCAFFOLD agent. Create the buildable project skeleton so the parallel module agents have a firm contract. Do ALL of:
1. git is already initialized. Create root configs: package.json (React 18, TypeScript, Vite, Tailwind, @tauri-apps/api v2, @tauri-apps/cli v2, zustand, vitest + @testing-library/react), tsconfig.json, vite.config.ts, tailwind.config.js, postcss.config.js, index.html.
2. src-tauri/: Cargo.toml (tauri v2, rusqlite {features=["bundled"]}, serde, serde_json, tokio, reqwest, hound, cpal, keyring, uuid, anyhow, thiserror; OPTIONAL deps whisper-rs behind feature "whisper", sherpa-onnx (or its rust binding) behind feature "diarize"; windows-only deps 'wasapi' and 'windows' under [target.'cfg(windows)'.dependencies]); tauri.conf.json (Tauri 2 schema, app identifier com.meetingrecordapp.app, system tray enabled, window config); build.rs.
3. src-tauri/src/main.rs (Tauri entry, calls lib run()), lib.rs (declare: pub mod models; pub mod storage; pub mod export; pub mod ai; pub mod audio; pub mod transcription; pub mod detection; pub mod commands; pub mod tray; plus pub mod video with a // v1.1 stub; a run() that registers a tauri::generate_handler! with command stubs — keep it minimal, the Integrate phase wires it fully), models.rs (the SHARED domain types matching the DB schema: Meeting, MediaFile, TranscriptSegment, Summary, ActionItem, KeyDecision, Settings enums MeetingStatus/MeetingType — derive Serialize/Deserialize/Clone/Debug). This models.rs is the contract; make it complete.
4. src-tauri/migrations/001_initial.sql — copy the schema from PRD §4.3 INCLUDING the FTS5 external-content sync triggers (transcript_segments_ai/ad/au).
5. Stub mod.rs files for storage/export/ai/audio/transcription/detection (just 'pub' module decls + a doc comment) and an empty commands.rs/tray.rs/video/mod.rs so the tree exists. Module agents will fill them; keep stubs tiny to minimize collisions.
6. src/main.tsx, src/App.tsx (minimal shell), src/styles/globals.css (Tailwind directives). .env.example (AI_PROVIDER=ollama, OPENAI_API_KEY=, etc.). README.md (build instructions incl. 'install Rust via rustup' since cargo is absent).
7. Run 'npm install' then verify the frontend shell builds ('npm run build'); report the actual result. Do NOT run cargo (absent).
Return the structured result.`,
  { label: 'scaffold', phase: 'Scaffold', schema: IMPL_SCHEMA },
)

log('✅ Scaffold 完成。開始平行實作各模組(每個 agent 負責不重疊的目錄)。')

// ============================================================
// Implement each module in its own dir, review as soon as it's done.
const built = await pipeline(
  MODULES,
  (m) => agent(
    `${DECISIONS}

You are the IMPLEMENT agent for the **${m.key}** module (owns ${m.dir}). The scaffold already created configs, src-tauri/src/lib.rs, src-tauri/src/models.rs (shared types — IMPORT from crate::models, never redefine), and migrations. Read them first.

YOUR TASK:
${m.spec}

CONSTRAINTS: Only write files under ${m.dir} (frontend agent: under src/, never src-tauri/). Do NOT edit lib.rs or models.rs (if you truly need a new shared type, note it in 'notes' for the Integrate phase instead). Write real, idiomatic code + #[cfg(test)] (Rust) / vitest (frontend) tests. Run whatever you CAN (frontend: npm run build/test). You CANNOT run cargo (absent) — write the tests anyway and say clearly they're unrun. Return the structured result with accurate public_api signatures.`,
    { label: `impl:${m.key}`, phase: 'Implement', schema: IMPL_SCHEMA },
  ),
  (impl, m) => agent(
    `${DECISIONS}

You are an adversarial REVIEWER for the **${m.key}** module. The implementer reported:
${JSON.stringify(impl, null, 2)}

Read the actual files under ${m.dir}. Check, skeptically: (a) does it match the PRD decisions (Windows-first, audio-only, Ollama-default, whisper-rs, FTS triggers, feature gates)? (b) given there is NO live Rust compiler, would it plausibly compile — wrong imports, type mismatches vs crate::models, missing cfg gates, async misuse, unused Result? (c) are the tests meaningful (not asserting trivialities)? (d) any invented scope or security issue (keys in plaintext, sql injection)? Be concrete with file + line-ish locations. If frontend, confirm 'npm run build' actually passed. Return the structured verdict.`,
    { label: `review:${m.key}`, phase: 'Review', schema: REVIEW_SCHEMA },
  ),
)

const reviews = built.filter(Boolean)
const needsFix = reviews.filter((r) => r?.verdict === 'needs_fix')
log(`🔎 Review 完成:${reviews.length} 模組,${needsFix.length} 個被標記 needs_fix。`)

// ============================================================
phase('Integrate')
log('🔗 Phase 4/4 — 整合 Tauri commands/tray + lib.rs invoke handler,並做最終驗證 + 完整性報告')

const COMPLETENESS_SCHEMA = {
  type: 'object',
  additionalProperties: false,
  required: ['frontend_build', 'rust_status', 'wired_commands', 'remaining_work', 'needs_human', 'summary'],
  properties: {
    frontend_build: { type: 'string', description: 'exact result of npm run build (passed/failed + key errors)' },
    rust_status: { type: 'string', description: 'cargo check/test result, or "cargo absent — Rust unverified"' },
    wired_commands: { type: 'array', items: { type: 'string' } },
    remaining_work: { type: 'array', items: { type: 'string' }, description: 'what is stubbed / per phase still to do' },
    needs_human: { type: 'array', items: { type: 'string' }, description: 'blocked on Windows hardware / API keys / model downloads / rust install' },
    summary: { type: 'string' },
  },
}

const integrate = await agent(
  `${DECISIONS}

You are the INTEGRATE + VERIFY agent. All module agents have written their dirs. Module reviews:
${JSON.stringify(reviews.map((r) => ({ module: r?.module, verdict: r?.verdict, blockers: (r?.issues || []).filter((i) => i.severity === 'blocker') })), null, 2)}

DO:
1. Read every module's actual public API (storage/export/ai/audio/transcription/detection) and write src-tauri/src/commands.rs (Tauri #[tauri::command] handlers: start/stop/pause/resume recording, list/get/delete meetings, transcribe, summarize, search, export, get/set settings, storage usage) + src-tauri/src/tray.rs (system tray w/ recording status + quick controls), then update lib.rs's generate_handler! to register them. Fix obvious cross-module mismatches you can see (imports, type names vs crate::models). Apply any "needs a new shared type" notes by adding them to models.rs.
2. Fix any BLOCKER issues the reviewers found that you can resolve by reading the code.
3. Verify: run 'npm run build' and report the exact result. Attempt 'cargo --version'; if absent, state Rust is unverified (do NOT claim it compiles). If present, run 'cargo check' and 'cargo test' and report.
4. Produce a completeness report: what's done, what's stubbed per PRD phase, and what is blocked on a human (Rust install via rustup, a Windows machine for audio/WASAPI, API keys, whisper/sherpa model downloads).
Be brutally honest. Return the structured report.`,
  { label: 'integrate+verify', phase: 'Integrate', schema: COMPLETENESS_SCHEMA, effort: 'high' },
)

log('🏁 Workflow 完成。')

return {
  scaffold: { files: scaffold?.files_written?.length ?? 0, unverified: scaffold?.unverified ?? [] },
  modules: reviews.map((r) => ({ module: r?.module, verdict: r?.verdict, compiles_likely: r?.compiles_likely })),
  integrate,
}
