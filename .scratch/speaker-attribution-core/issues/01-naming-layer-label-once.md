# 01 — 命名層端到端(speaker_labels + 標一次)

Status: done
Type: AFK

Spec: `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md` §C3

## What to build

一條「把 diarization 的『Speaker N』改標成真名、標一次全稿套用」的端到端命名層。

原始 speaker 值(diarization/Gemini 寫入的 "Speaker 1" 等)保持不動;新增一張 `speaker_labels` 匯流表記錄每個 meeting 的「原始 label → 顯示名」;讀取逐字稿時用它覆蓋 `speaker` 欄位(非破壞、可重命名)。前端讓使用者點逐字稿上的發話人 chip、輸入名字,同一 meeting 內該群所有段落即時更新。

整條在 **mock 模式即可 demo**,不需 Windows / diarize —— mock 資料給帶 "Speaker 1/2" 的段落,改名後全稿更新。

## Acceptance criteria

- [x] migration 建 `speaker_labels(meeting_id, raw_label, display_name, source)`,PK `(meeting_id, raw_label)`;加法式、`IF NOT EXISTS`、冪等、無 version table(`003_speaker_labels.sql`)
- [x] 後端指令 `set_speaker_label(meeting_id, raw_label, display_name)`:upsert 一列,`source='manual'`;註冊進 `generate_handler!`
- [x] 解析:segments 保留原始 `speaker`;新增 `get_speaker_labels` 指令,前端以 `labels[raw] ?? raw` 解析(改 spec C3 的 overlay-at-read → 前端解析,理由:二次改名正確性)
- [x] `tauri.ts` COMMANDS + `mockInvoke`(有狀態 label store);`mocks.ts` 改為 "Speaker 1/2"
- [x] 前端:`Transcript` speaker chip 可點 → 輸入名字 → `onRelabel` 重載 → 全段即時更新
- [x] 測試:`set_speaker_label` roundtrip/upsert(Rust)、`Transcript` 解析 + rename(vitest)、migration 建表
- [x] `npm run build` 與 `cargo test`(預設 features)全綠

## Blocked by

None - can start immediately.

## Comments

**Done (2026-07-04).** TDD red-green throughout. backend `cargo test` 252 綠、frontend 43 綠、`npm run build` 綠。
一處對 spec 的偏離:C3 原設計 overlay-at-read(後端覆寫 `speaker`),實作改為**前端解析 + 保留原始 label**,因為 overlay 會讓「改名後再改名」找不到原始 key。spec C3 已同步更新。
export/summary 顯示真名未做(各自出口點解析,列後續)。
