# MeetingRecordApp — Project Structure

```
MettingRecordApp/
├── docs/
│   └── PRD.md                    # Product Requirements Document
├── src-tauri/                    # Rust backend (Tauri)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── src/
│   │   ├── main.rs               # Tauri entry point
│   │   ├── lib.rs                # Module exports
│   │   ├── audio/
│   │   │   ├── mod.rs            # Audio capture module
│   │   │   ├── microphone.rs     # cpal-based mic capture
│   │   │   ├── system_audio.rs   # WASAPI loopback (v1.0: Windows only)
│   │   │   ├── mixer.rs          # Combine mic + system audio
│   │   │   └── recorder.rs       # Recording orchestrator
│   │   ├── video/                 # (v1.1 — NOT in v1.0)
│   │   │   ├── mod.rs            # Screen recording module
│   │   │   ├── ffmpeg.rs         # FFmpeg wrapper
│   │   │   └── selector.rs       # Window/screen selection
│   │   ├── transcription/
│   │   │   ├── mod.rs            # Transcription module
│   │   │   ├── whisper.rs        # whisper.cpp via whisper-rs
│   │   │   ├── model.rs          # Model download & management
│   │   │   ├── diarization.rs    # sherpa-onnx speaker diarization
│   │   │   └── processor.rs      # Audio → transcript pipeline
│   │   ├── ai/
│   │   │   ├── mod.rs            # AI summarization module
│   │   │   ├── provider.rs       # Provider trait definition (local + cloud)
│   │   │   ├── ollama.rs         # Local LLM (Ollama) — default, private
│   │   │   ├── openai.rs         # OpenAI API client
│   │   │   ├── claude.rs         # Claude API client
│   │   │   ├── gemini.rs         # Gemini API client
│   │   │   └── templates.rs      # Summary templates
│   │   ├── storage/
│   │   │   ├── mod.rs            # Storage module
│   │   │   ├── database.rs       # SQLite operations
│   │   │   ├── files.rs          # File system management
│   │   │   └── search.rs         # FTS5 search
│   │   ├── detection/
│   │   │   ├── mod.rs            # Window detection module
│   │   │   └── monitor.rs        # Active window monitor
│   │   ├── export/
│   │   │   ├── mod.rs            # Export module
│   │   │   ├── markdown.rs       # Markdown exporter
│   │   │   ├── pdf.rs            # PDF exporter
│   │   │   ├── srt.rs            # SRT/VTT exporter
│   │   │   ├── json.rs           # JSON exporter
│   │   │   └── notion.rs         # Notion format exporter
│   │   ├── tray.rs               # System tray management
│   │   └── commands.rs           # Tauri command handlers
│   └── migrations/               # SQLite migrations
│       └── 001_initial.sql
├── src/                           # React frontend
│   ├── main.tsx                   # React entry point
│   ├── App.tsx                    # Main app component
│   ├── components/
│   │   ├── Tray/
│   │   │   ├── TrayIcon.tsx      # System tray icon
│   │   │   └── TrayMenu.tsx      # Tray context menu
│   │   ├── Floating/
│   │   │   ├── MiniPanel.tsx     # Floating recording panel
│   │   │   └── AudioLevel.tsx    # Audio level indicator
│   │   ├── Meeting/
│   │   │   ├── MeetingList.tsx   # Meeting history list
│   │   │   ├── MeetingDetail.tsx # Meeting detail view
│   │   │   ├── MeetingPlayer.tsx # Audio/video player
│   │   │   └── Transcript.tsx    # Transcript viewer/editor
│   │   ├── Summary/
│   │   │   ├── SummaryView.tsx   # AI summary display
│   │   │   ├── ActionItems.tsx   # Action items list
│   │   │   └── SummaryEditor.tsx # Custom prompt editor
│   │   ├── Export/
│   │   │   └── ExportDialog.tsx  # Export options dialog
│   │   ├── Settings/
│   │   │   ├── SettingsPage.tsx  # Settings view
│   │   │   ├── AudioSettings.tsx # Audio device config
│   │   │   ├── AISettings.tsx    # API key & model config
│   │   │   └── GeneralSettings.tsx # General preferences
│   │   └── common/
│   │       ├── Button.tsx
│   │       ├── Modal.tsx
│   │       └── Tooltip.tsx
│   ├── hooks/
│   │   ├── useRecording.ts       # Recording state hook
│   │   ├── useTranscription.ts   # Transcription status hook
│   │   └── useMeetings.ts        # Meeting data hook
│   ├── stores/
│   │   ├── recordingStore.ts     # Recording state management
│   │   └── settingsStore.ts      # Settings state
│   ├── lib/
│   │   ├── tauri.ts              # Tauri invoke wrappers
│   │   └── constants.ts          # App constants
│   └── styles/
│       └── globals.css           # Global styles + Tailwind
├── public/
│   └── icons/                    # App icons
├── package.json
├── tsconfig.json
├── tailwind.config.js
├── vite.config.ts
└── README.md
```

## Key Files to Create First

> **Phase 0 (最先做):** `src-tauri/src/audio/system_audio.rs` — Windows WASAPI loopback PoC（+ `audio/microphone.rs` + `audio/mixer.rs`），先證明「擷取系統音訊 + 麥克風 + 混音 + 寫 WAV」可行，再建後續模組。

1. `src-tauri/Cargo.toml` — Rust dependencies
2. `src-tauri/tauri.conf.json` — Tauri configuration
3. `src-tauri/migrations/001_initial.sql` — Database schema
4. `src/main.tsx` — React entry point
5. `package.json` — Node.js dependencies
