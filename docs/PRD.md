# MeetingRecordApp — Product Requirements Document

> **Version:** 1.0  
> **Date:** 2026-06-18  
> **Status:** Draft  
> **Author:** 黃士桓 + 碼姬 (AI)

> **Note (2026-06-18 修訂):** 本 PRD 已收斂 v1.0 範圍 — **Windows 優先、音訊 only、本地優先摘要(Ollama)+ 雲端可選、基礎 diarization(sherpa-onnx)**；macOS/Linux 與螢幕錄製移至 v1.1。另:專案資料夾名 `MettingRecordApp` 拼字錯誤(Metting→Meeting),建議趁 greenfield 改名。

---

## 1. Problem Statement

在現代遠端/混合工作環境中，會議記錄是團隊協作的痛點：
- 參會者在會議中忙著記筆記，無法專注討論
- 會議結束後，遺漏重要決策和 Action Items
- 手動整理逐字稿耗時且容易遺漏
- 現有方案（如 Notion AI）綁定特定平台，不夠靈活
- 企業環境中 Microsoft Teams 和 Google Meet 並存，缺乏統一的記錄工具
- 錄音檔散落各處，難以搜尋和管理

**核心痛點：** 開會時要專心聽，但又要記筆記，兩者互相干擾。會後整理更是噩夢。

---

## 2. Solution

MeetingRecordApp 是一款桌面應用（v1.0 聚焦 Windows，macOS/Linux 為 v1.1+ 目標），提供：
- **系統級音訊捕捉**：不加入會議室，直接抓取系統音訊 + 麥克風
- **可選螢幕錄製（v1.1）**：FFmpeg 驅動的高品質螢幕/視窗錄製
- **本地 AI 逐字稿**：whisper.cpp（透過 `whisper-rs`），支援多語言（含中文）；預設 small/base 起步，medium/large-v3 可選
- **基礎語者分離（diarization）**：sherpa-onnx 標注 who-said-what
- **本地優先 AI 摘要**：預設可用本地 LLM（Ollama）產生結構化摘要、待辦事項、決策清單；雲端 API（OpenAI/Claude/Gemini）為可選的高品質選項
- **不綁會議平台 API**：不需要 Teams/Google Meet 的企業級授權
- **本地儲存**：錄音與逐字稿完全本地；若選用雲端摘要，僅該次逐字稿會在明確告知後送出
- **系統托盤常駐**：會議中不干擾，點擊即可查看進度

**類比：** Notion AI Meeting Notes 的核心體驗 + 本地優先（摘要可全本地）+ 不綁會議平台。

---

## 3. User Stories

### 3.1 錄音核心功能

1. As a user, I want to click a button to start recording my meeting, so that I can focus on the discussion without taking notes
2. As a user, I want to stop recording with one click, so that I can quickly end the session when the meeting finishes
3. As a user, I want to pause and resume recording, so that I can handle interruptions without losing context
4. As a user, I want to see the recording duration in the system tray, so that I know the app is actively recording
5. As a user, I want to capture both system audio and microphone audio simultaneously, so that both my voice and the meeting audio are recorded
6. As a user, I want to select which audio input devices to use, so that I can choose the right microphone and speakers
7. As a user, I want the app to auto-detect when Teams or Google Meet windows are active and prompt me to start recording, so that I don't forget to record

### 3.2 螢幕錄製功能 (v1.1)

8. As a user, I want to optionally enable screen recording alongside audio, so that I have a visual reference of the meeting
9. As a user, I want to choose between recording the full screen or a specific window, so that I only capture what's relevant
10. As a user, I want to configure video quality settings (resolution, frame rate), so that I can balance quality and file size
11. As a user, I want screen recording to be disabled by default to save resources, so that audio-only recording is the lightweight default

### 3.3 逐字稿功能

12. As a user, I want the app to automatically generate a transcript after recording stops, so that I don't need to wait for manual processing
13. As a user, I want the transcript to include timestamps for each segment, so that I can jump to specific moments in the recording
14. As a user, I want the transcript to support multiple languages (Traditional Chinese, Simplified Chinese, English, Japanese), so that I can use it for international meetings
15. As a user, I want to see the transcription progress (a progress indicator after recording stops — NOT live in-meeting transcription), so that I know how long to wait
16. As a user, I want to edit the transcript after generation, so that I can correct any transcription errors
17. As a user, I want speaker diarization (who said what), so that I can identify different speakers in the meeting *(v1.0, basic — sherpa-onnx)*

### 3.4 AI 摘要功能

18. As a user, I want an automatic meeting summary generated after transcription, so that I can quickly grasp the key points
19. As a user, I want the summary to extract action items with owners and deadlines, so that follow-ups are clear
20. As a user, I want the summary to highlight key decisions made during the meeting, so that nothing falls through the cracks
21. As a user, I want customizable summary templates for different meeting types (1:1, team sync, client call, interview), so that the summary matches my needs
22. As a user, I want to regenerate or refine the summary with custom prompts, so that I can get exactly what I need
23. As a user, I want the AI to suggest follow-up questions based on the meeting content, so that I can prepare for the next discussion

### 3.5 資料管理

24. As a user, I want all recordings, transcripts, and summaries stored locally, so that my data stays private
25. As a user, I want to browse my meeting history in a list view, so that I can find past meetings quickly
26. As a user, I want to search across all transcripts by keyword, so that I can find specific discussions
27. As a user, I want to tag meetings with custom labels (project name, team, etc.), so that I can organize them
28. As a user, I want to view a meeting's detail page showing recording player, transcript, and summary together, so that everything is in one place
29. As a user, I want to delete recordings I no longer need, so that I can manage disk space
30. As a user, I want to see the total storage used by the app, so that I can manage my disk space proactively

### 3.6 匯出功能

31. As a user, I want to export a meeting record as Markdown, so that I can paste it into any document
32. As a user, I want to export as PDF with formatted transcript and summary, so that I can share it professionally
33. As a user, I want to export transcript as SRT/VTT subtitle file, so that I can use it in video editing
34. As a user, I want to export as JSON for integration with other tools, so that I can build custom workflows
35. As a user, I want to export to Notion page format, so that I can import it into my existing workspace
36. As a user, I want batch export of multiple meetings, so that I can archive them efficiently

### 3.7 使用者介面

37. As a user, I want a system tray icon that shows recording status, so that I always know if the app is active
38. As a user, I want a floating mini-panel during recording with pause/stop controls, so that I can control recording without leaving my meeting
39. As a user, I want a full app window for browsing history and viewing details, so that I have a comfortable interface for deep work
40. As a user, I want dark mode support, so that I can use the app comfortably in low-light environments
41. As a user, I want keyboard shortcuts for common actions (start/stop recording), so that I can be more efficient
42. As a user, I want the app to remember my last used settings, so that I don't need to reconfigure every time

### 3.8 設定與配置

43. As a user, I want to configure default audio input/output devices, so that recording works correctly out of the box
44. As a user, I want to configure the whisper model size, so that I can balance speed and accuracy
45. As a user, I want to set my API key for AI summarization, so that the app can generate summaries
46. As a user, I want to choose which AI provider to use — local Ollama (default, private) or cloud (OpenAI, Claude, Gemini) — so that I'm not locked in and can keep my transcript fully local if I want
47. As a user, I want to configure auto-start behavior (start with OS, minimize to tray), so that the app fits my workflow
48. As a user, I want to set a default storage location for recordings, so that I can manage disk usage

### 3.9 跨平台

49. As a macOS user, I want the app to work seamlessly with CoreAudio and ScreenCaptureKit, so that recording works natively *(v1.1)*
50. As a Windows user, I want the app to work with WASAPI loopback and Windows Audio Session API, so that system audio capture works *(v1.0)*
51. As a Linux user, I want the app to work with PipeWire/PulseAudio, so that it works on my preferred OS *(v1.1+)*
52. As a user, I want consistent UI/UX across all platforms, so that I have the same experience everywhere

---

## 4. Implementation Decisions

### 4.1 Technology Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| **App Framework** | Tauri 2.0 | 小型本體、native webview、Rust backend（注:whisper ~1.5GB 與 sherpa-onnx 模型按需下載,實際磁碟佔用遠大於本體,勿宣稱「~2MB」） |
| **Frontend** | React + TypeScript + Tailwind CSS | Mature ecosystem, fast development |
| **Backend** | Rust | Performance, memory safety, cross-platform audio APIs |
| **Audio Capture** | `cpal` crate (Rust) | Cross-platform audio library for microphone |
| **System Audio** | WASAPI Loopback (`wasapi` crate) | v1.0 僅 Windows；macOS (CoreAudio/ScreenCaptureKit)、Linux (PipeWire) 為 v1.1+ |
| **Screen Recording** (v1.1) | FFmpeg (sidecar) | Industry standard;因體積/授權/A-V 同步複雜度延到 v1.1 |
| **Transcription** | whisper.cpp via `whisper-rs` | 現成、維護中的 Rust 綁定,免手刻 bindgen |
| **Diarization** | sherpa-onnx | segmentation + speaker embedding（ONNX runtime,C API→Rust FFI,無 Python） |
| **Database** | SQLite (via `rusqlite`) | Local, zero-config, reliable |
| **AI Summarization** | 本地 Ollama（預設）或雲端 API（可選） | 本地優先保隱私;雲端 OpenAI/Claude/Gemini 為可選高品質選項 |
| **Local LLM** | Ollama (HTTP API) | 本地摘要,免額外打包 runtime |
| **UI Framework** | React + Tailwind CSS | Fast development, good desktop app support |
| **Build System** | Tauri CLI + cargo | Integrated build and packaging |

### 4.2 Architecture

```
┌─────────────────────────────────────────────────────┐
│                  Tauri App Shell                     │
├─────────────────────────────────────────────────────┤
│  Frontend (React + Tailwind)                        │
│  ├── Tray Panel (recording controls, status)        │
│  ├── Floating Mini-Panel (quick access)             │
│  ├── Main Window (history, detail, settings)        │
│  └── Settings UI                                    │
├─────────────────────────────────────────────────────┤
│  Rust Backend                                       │
│  ├── Audio Capture Module                           │
│  │   ├── Microphone (cpal)                          │
│  │   ├── System Audio (platform loopback)           │
│  │   └── Audio Mixer (combine mic + system)         │
│  ├── Screen Recording Module  (v1.1, not in v1.0)   │
│  │   ├── FFmpeg Wrapper                             │
│  │   └── Window/Screen Selection                    │
│  ├── Transcription Module                           │
│  │   ├── whisper.cpp (whisper-rs)                   │
│  │   ├── Diarization (sherpa-onnx)                  │
│  │   ├── Model Manager (download, cache)            │
│  │   └── Progress Reporting (batch, post-record)    │
│  ├── AI Summarization Module                        │
│  │   ├── Provider Abstraction (Ollama/OpenAI/Claude)│
│  │   ├── Template Engine                            │
│  │   └── Action Item Extraction                     │
│  ├── Storage Module                                 │
│  │   ├── SQLite Database                            │
│  │   ├── File System (audio/video files)            │
│  │   └── Search Index (FTS5)                        │
│  ├── Window Detection Module                        │
│  │   ├── Active Window Monitor                      │
│  │   └── Meeting App Identification                 │
│  └── Export Module                                  │
│      ├── Markdown Generator                         │
│      ├── PDF Generator                              │
│      ├── SRT/VTT Generator                          │
│      ├── JSON Exporter                              │
│      └── Notion Format Exporter                     │
└─────────────────────────────────────────────────────┘
```

### 4.3 Data Model

```sql
-- Core meeting record
CREATE TABLE meetings (
    id TEXT PRIMARY KEY,          -- UUID
    title TEXT,                   -- Auto-detected or user-set
    start_time DATETIME NOT NULL,
    end_time DATETIME,
    duration_seconds INTEGER,
    status TEXT DEFAULT 'recording', -- recording, transcribing, completed, error
    tags TEXT,                    -- JSON array of tags
    meeting_type TEXT,            -- 1on1, team_sync, client_call, interview, other
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Audio/video files
CREATE TABLE media_files (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    file_type TEXT NOT NULL,      -- audio, video
    file_path TEXT NOT NULL,
    file_size_bytes INTEGER,
    format TEXT,                  -- wav, mp3, mp4, webm
    duration_seconds INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Transcript segments
CREATE TABLE transcript_segments (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    segment_index INTEGER NOT NULL,
    start_time_ms INTEGER NOT NULL,
    end_time_ms INTEGER NOT NULL,
    text TEXT NOT NULL,
    speaker TEXT,                 -- Speaker label if diarization enabled
    confidence REAL,
    language TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- AI summary
CREATE TABLE summaries (
    id TEXT PRIMARY KEY,
    meeting_id TEXT REFERENCES meetings(id),
    summary_type TEXT NOT NULL,   -- auto, custom, template
    content TEXT NOT NULL,        -- Markdown formatted summary
    action_items TEXT,            -- JSON array of {owner, task, deadline}
    key_decisions TEXT,           -- JSON array of decisions
    prompt_used TEXT,             -- The prompt that generated this summary
    ai_provider TEXT,             -- openai, claude, gemini
    ai_model TEXT,                -- specific model used
    tokens_used INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Settings
CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Full-text search index
CREATE VIRTUAL TABLE transcript_fts USING fts5(
    text, speaker, language,
    content='transcript_segments',
    content_rowid='rowid'
);

-- FTS5 external-content sync triggers (REQUIRED — keep transcript_fts in sync)
CREATE TRIGGER transcript_segments_ai AFTER INSERT ON transcript_segments BEGIN
    INSERT INTO transcript_fts(rowid, text, speaker, language)
    VALUES (new.rowid, new.text, new.speaker, new.language);
END;
CREATE TRIGGER transcript_segments_ad AFTER DELETE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text, speaker, language)
    VALUES ('delete', old.rowid, old.text, old.speaker, old.language);
END;
CREATE TRIGGER transcript_segments_au AFTER UPDATE ON transcript_segments BEGIN
    INSERT INTO transcript_fts(transcript_fts, rowid, text, speaker, language)
    VALUES ('delete', old.rowid, old.text, old.speaker, old.language);
    INSERT INTO transcript_fts(rowid, text, speaker, language)
    VALUES (new.rowid, new.text, new.speaker, new.language);
END;
```

### 4.4 Audio Capture Architecture

**Microphone Capture (cpal):**
- Default input device detection
- Configurable sample rate (16kHz for whisper, 44.1kHz for recording)
- Real-time audio level monitoring

**System Audio Loopback:**
- **Windows (v1.0):** WASAPI Loopback mode via `wasapi` crate ← 唯一在 v1.0 範圍內
- **macOS (v1.1):** ScreenCaptureKit (macOS 13+，需 Screen Recording TCC 權限；Rust 生態不成熟)
- **Linux (v1.1+):** PipeWire monitor source or PulseAudio monitor

**Audio Mixing:**
- ⚠️ 麥克風與系統音訊來自**不同裝置 / 不同 clock domain → sample-rate 漂移**，必須處理:
  重採樣到共同 sample rate + 處理 buffer under/overrun，避免長時間累積失步(此為核心風險,Phase 0 spike 就要驗證)
- 轉錄路徑:混合為 16kHz mono 餵 whisper
- 錄音保存:可選保留雙軌(系統 / 麥克風)供 diarization 輔助(取代原「L/R 硬分軌」的 hack)

### 4.5 Transcription Pipeline

```
Recording stops
    ↓
Audio file saved (WAV/FLAC)
    ↓
whisper.cpp (whisper-rs) → segments with timestamps + text
    ↓
sherpa-onnx diarization → assign speaker label to each segment
    ↓
Save to SQLite (transcript_segments, incl. speaker)
    ↓
Trigger AI summary (if enabled)
```

**whisper.cpp Integration (`whisper-rs`):**
- 使用現成、維護中的 `whisper-rs` 綁定（免手刻 bindgen / FFI）
- Model download on first use (or bundled)
- Support for: tiny, base, small, medium, large-v3
- **預設 small/base 起步**（速度與體積友善）；medium/large-v3 為可選（繁中品質更佳但較慢、較大）
- GPU acceleration: v1.0 以 Windows 為主（CUDA / Vulkan / DirectML 視可用性）；Metal (macOS) 為 v1.1

**Diarization (sherpa-onnx):**
- segmentation + speaker embedding 模型（ONNX runtime，C API → Rust FFI，無 Python）
- 與 whisper segments 對齊後寫入 `transcript_segments.speaker`
- 需額外管理一組模型下載 / 快取（同 Model Manager）；定位為「基礎版」，準確度有限

### 4.6 AI Summarization Pipeline

```
Transcript complete
    ↓
Build prompt with meeting context + transcript
    ↓
Send to configured AI provider
    ↓
Parse structured response (summary, action items, decisions)
    ↓
Render in UI + save to database
```

**Provider Abstraction:**
```rust
trait AiProvider {
    async fn summarize(&self, transcript: &str, template: &SummaryTemplate) -> Result<Summary>;
    fn name(&self) -> &str;
    fn models(&self) -> Vec<String>;
}

struct OllamaProvider { endpoint: String, model: String } // local, DEFAULT, no API key
struct OpenAiProvider { api_key: String, model: String }
struct ClaudeProvider { api_key: String, model: String }
struct GeminiProvider { api_key: String, model: String }
```

**本地優先:** 預設使用本地 Ollama（逐字稿不離開機器）;雲端 provider 為可選的高品質選項,送出前明確告知逐字稿會上雲。

**長逐字稿處理:** 超過模型 context window 時採 **chunking + map-reduce**（先分段摘要再彙整）,避免長會議摘要失敗 / 截斷。

**成本估算:** 使用雲端 provider 時,送出前顯示概估 token 數與費用;本地 Ollama 無此成本。

**Summary Templates:**
- **1:1 Meeting:** Discussion topics, action items, follow-ups
- **Team Sync:** Updates from each member, blockers, action items
- **Client Call:** Client needs, proposed solutions, next steps
- **Interview:** Candidate assessment, strengths/weaknesses, hiring recommendation
- **General:** Key points, decisions, action items, open questions

### 4.7 Window Detection

- **Windows (v1.0):** `GetForegroundWindow` + `GetWindowText`
- **macOS (v1.1):** `CGWindowListCopyWindowInfo` via Core Graphics（注:新版 macOS 讀視窗標題需 Screen Recording TCC 權限）
- **Linux (v1.1+):** `xdotool` or `libwnck`
- Match against known meeting app window titles/classes:
  - Microsoft Teams: "Microsoft Teams"
  - Google Meet (Chrome): "Google Meet" or meeting URL pattern
  - Zoom: "Zoom Meeting"
  - (Extensible pattern list)

### 4.8 Export Format Specs

**Markdown:**
```markdown
# Meeting: [Title]
**Date:** 2026-06-18 14:00 - 15:30  
**Duration:** 1h 30m  
**Type:** Team Sync

## Summary
[AI-generated summary]

## Action Items
- [ ] [Owner] [Task] - Due: [Date]

## Key Decisions
1. [Decision 1]
2. [Decision 2]

## Transcript
[14:00:00] Speaker A: [text]
[14:00:05] Speaker B: [text]
```

**PDF:** Markdown rendered with CSS styling, embedded audio player link

**SRT/VTT:**
```
1
00:00:00,000 --> 00:00:05,000
[Speaker A] Hello everyone

2
00:00:05,000 --> 00:00:10,000
[Speaker B] Let's start the meeting
```

**JSON:** Full structured data export (meeting, transcript, summary, metadata)

**Notion Format:** Markdown with Notion-compatible blocks

---

## 5. Testing Decisions

### 5.1 Testing Philosophy

- **External behavior only:** Test what the app does, not how it does it
- **Integration tests over unit tests:** Focus on real-world workflows
- **Platform-specific tests:** Each OS gets dedicated test runs

### 5.2 Test Categories

| Category | Framework | Scope |
|----------|-----------|-------|
| **Frontend** | Vitest + React Testing Library | UI components, state management |
| **Rust Backend** | `cargo test` | Audio processing, storage, transcription |
| **Integration** | Playwright (Tauri) | End-to-end workflows |
| **Audio** | Custom test harness | Audio capture, mixing, format conversion |
| **Transcription** | Golden files | Input audio → expected transcript |
| **Export** | Snapshot tests | Meeting data → expected export format |

### 5.3 Key Test Scenarios

1. Start recording → capture 30s audio → stop → verify WAV file created
2. Record with screen → verify video file created with correct dimensions
3. Transcribe audio → verify transcript segments with timestamps
4. Generate summary → verify structured output with action items
5. Export to Markdown → verify format matches spec
6. Search transcripts → verify keyword matching works
7. Window detection → verify meeting app is identified correctly
8. Settings persistence → verify settings survive app restart

---

## 6. Out of Scope

### 6.1 MVP (v1.0) — NOT included

- Cloud sync / account system
- Mobile apps (iOS/Android) — Tauri supports it but not in MVP
- Video conferencing integration (bot joining meetings)
- Real-time streaming transcription during meeting (v1.0 是錄完批次轉錄 + 進度指示)
- Screen recording (v1.1 — v1.0 是 audio-only)
- macOS / Linux support (v1.0 是 Windows-only;macOS/Linux 為 v1.1+)
- Calendar integration (v1.1)
- Multi-language UI (v1.0 is Traditional Chinese + English)
- Team collaboration features
- API for third-party integrations
- Webhooks / notifications

> 注:Speaker diarization 與本地 LLM (Ollama) **已納入 v1.0**（見 §3.3 / §3.8 / §4.5 / §4.6），不在排除清單。

### 6.2 Deferred to Later Versions

| Feature | Target Version |
|---------|---------------|
| Screen recording | v1.1 |
| macOS support | v1.1 |
| Linux support | v1.1+ |
| Calendar sync (Google/Outlook) | v1.1 |
| Real-time (in-meeting) transcription | v1.2 |
| Mobile apps | v2.0 |
| Cloud sync | v2.0 |
| Team collaboration | v2.0 |
| Notion API integration | v1.2 |

---

## 7. Further Notes

### 7.1 Performance Targets

| Metric | Target |
|--------|--------|
| App launch to ready | < 3 seconds |
| Recording start latency | < 500ms |
| Transcription speed | 優於 realtime（含 GPU 加速的典型 Windows 桌機/筆電；視 whisper 模型大小與 GPU 而定）|
| Summary generation | < 10 seconds |
| Memory usage (idle) | < 100MB |
| Memory usage (recording) | < 300MB |
| Disk usage (1hr audio-only) | ~100MB |
| Disk usage (1hr with video) | ~500MB-2GB |

### 7.2 Security Considerations

- All data stored locally only (no telemetry without consent)
- API keys stored in OS keychain (not plain text)
- Audio files encrypted at rest (optional, v1.1)
- No network requests except AI API calls
- Open source (MIT License) for auditability

### 7.3 Development Phases

| Phase | Duration | Deliverables |
|-------|----------|--------------|
| **Phase 0: Audio Spike (最先做)** | 1 week | Windows WASAPI loopback + mic + 混音（含漂移處理）→ WAV 可行性驗證 |
| **Phase 1: Foundation** | 2 weeks | Tauri scaffold, UI shell, system tray (Windows) |
| **Phase 2: Audio Core** | 3 weeks | 麥克風 + 系統音訊擷取、漂移/混音、錄音控制（暫停/續錄）|
| **Phase 3: Transcription + Diarization** | 4 weeks | whisper-rs、模型管理、sherpa-onnx 語者分離 |
| **Phase 4: AI Summary** | 3 weeks | Provider 抽象（Ollama 本地 + 雲端可選）、模板、chunking、成本估算 |
| **Phase 5: Storage & Search** | 2 weeks | SQLite、FTS5(+triggers)、會議歷史 UI |
| **Phase 6: Export** | 1 week | Markdown, SRT/VTT, JSON（PDF 視情況）|
| **Phase 7: Polish** | 2 weeks | Dark mode、快捷鍵、設定、錯誤處理 |
| **Total v1.0 (Windows)** | ~18 weeks | 單平台、音訊 only |

> 注:原「~17 週」是 **3 平台同時** 的樂觀估計(不切實際)。此處 ~18 週為 **單平台(Windows)、audio-only** 的較實際估計,並已含 diarization + 本地 LLM。macOS/Linux、螢幕錄製為 v1.1 的額外工作。

### 7.4 Competitive Landscape

| Product | Strength | Weakness | Our Differentiator |
|---------|----------|----------|-------------------|
| Notion AI | Integrated with Notion | Requires Notion subscription, cloud-only | Local-first, no subscription |
| Otter.ai | Real-time transcription | Cloud-only, English-focused | Local processing, multilingual |
| Fireflies.ai | Good API integration | Expensive, cloud-only | Free, local, privacy |
| tl;dv | Video recording + transcripts | Chrome extension only | Full desktop app |
| Microsoft Copilot | Teams integration | Only works in Teams | Works with any meeting software |

### 7.5 Key Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| **系統音訊捕捉 (#1 風險)** | 整個錄音功能失效 | v1.0 聚焦 Windows WASAPI（最可控）；Phase 0 spike 先驗證；macOS/Linux 延後 v1.1 |
| 開發機 macOS、目標 Windows → 無法 dogfood | 回歸測試慢、易漏 bug | 準備 Windows 實機/VM（注意:VM 需設好 audio passthrough 才測得準 loopback）|
| 雙音源 clock drift / sample-rate 漂移 | 長會議音訊失步 | 重採樣 + buffer under/overrun 處理;Phase 0 spike 就驗證 |
| diarization 模型管線 | 多一組模型下載/管理、準確度有限 | sherpa-onnx 本地、模型快取;對外標示為「基礎版」|
| 長會議逐字稿超過 LLM context | 摘要失敗 / 截斷 | chunking + map-reduce |
| whisper 在較舊硬體太慢 | Poor UX | 預設 small/base,可選模型大小,背景處理 |
| 雲端 API 費用 | User churn | 預設本地 Ollama;雲端送出前顯示成本估算 |

---

*This PRD is a living document. Updates will be tracked in version control.*
