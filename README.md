# MeetingRecordApp

Local-first meeting recorder. Captures system audio + microphone, transcribes
locally (whisper.cpp), and summarizes with a local LLM (Ollama) by default —
cloud providers (OpenAI / Claude / Gemini) are optional.

> **v1.0 scope:** **Windows only**, **audio only**. macOS/Linux and screen
> recording are v1.1+. See [`docs/PRD.md`](docs/PRD.md) and
> [`docs/STRUCTURE.md`](docs/STRUCTURE.md).

---

## Stack

- **Shell:** Tauri 2.0
- **Frontend:** React 18 + TypeScript + Vite + Tailwind CSS (state: zustand)
- **Backend:** Rust — audio (`cpal` + `wasapi` loopback), transcription
  (`whisper-rs`, feature-gated), diarization (sherpa-onnx, feature-gated),
  storage (SQLite via `rusqlite` + FTS5), AI (`reqwest`)

## Prerequisites

- **Node.js** ≥ 18 and **npm** (this repo was scaffolded with Node 24 / npm 11).
- **Rust toolchain** — **required, install via [rustup](https://rustup.rs/)**:
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
  (On Windows use the `rustup-init.exe` installer.) `cargo` is not present by
  default; nothing in `src-tauri/` builds without it.
- **Tauri 2 system deps** — see the official
  [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/). On Windows
  this means the **WebView2 runtime** and the **MSVC build tools**.
- **Ollama** (optional but default AI provider) —
  [install Ollama](https://ollama.com/) and pull a model, e.g.
  `ollama pull llama3.1`.

## Setup

```sh
npm install
```

## Develop

Frontend only (Vite dev server):

```sh
npm run dev
```

Full app (Tauri window + Rust backend — requires the Rust toolchain):

```sh
npm run tauri dev
```

## Build

Frontend bundle:

```sh
npm run build      # tsc --noEmit + vite build → dist/
```

Full desktop app (Windows installer; requires Rust toolchain + Tauri deps):

```sh
npm run tauri build
```

### Cargo features

Heavy native ML libraries are **off by default** so the crate builds on a fresh
machine. Enable them when the native deps / models are available:

```sh
cargo build --features whisper           # whisper.cpp transcription
cargo build --features diarize           # sherpa-onnx diarization
cargo build --features full              # both
```

### App icons

`src-tauri/icons/` ships with a placeholder only. Generate the real icon set
(required by `tauri build`) from a 1024×1024 PNG:

```sh
npm run tauri icon path/to/app-icon.png
```

## Test

Frontend (Vitest + React Testing Library):

```sh
npm test
```

Backend (requires the Rust toolchain):

```sh
cd src-tauri && cargo test
```

## Project layout

See [`docs/STRUCTURE.md`](docs/STRUCTURE.md) for the full tree. Key paths:

- `src/` — React frontend
- `src-tauri/src/` — Rust backend modules (`audio`, `transcription`, `ai`,
  `storage`, `detection`, `export`, `commands`, `tray`; `video` is a v1.1 stub)
- `src-tauri/migrations/001_initial.sql` — SQLite schema (PRD §4.3)

## Privacy

All recordings, transcripts, and summaries stay on your machine. The only
network calls are to your chosen AI provider — and with the default (local
Ollama) there are none. Cloud summarization sends only that meeting's transcript,
after explicit confirmation. API keys live in the OS keychain, never in plain
text.

## License

MIT.
