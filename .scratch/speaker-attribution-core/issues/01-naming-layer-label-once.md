# 01 — 命名層端到端(speaker_labels + 標一次)

Status: ready-for-agent
Type: AFK

Spec: `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md` §C3

## What to build

一條「把 diarization 的『Speaker N』改標成真名、標一次全稿套用」的端到端命名層。

原始 speaker 值(diarization/Gemini 寫入的 "Speaker 1" 等)保持不動;新增一張 `speaker_labels` 匯流表記錄每個 meeting 的「原始 label → 顯示名」;讀取逐字稿時用它覆蓋 `speaker` 欄位(非破壞、可重命名)。前端讓使用者點逐字稿上的發話人 chip、輸入名字,同一 meeting 內該群所有段落即時更新。

整條在 **mock 模式即可 demo**,不需 Windows / diarize —— mock 資料給帶 "Speaker 1/2" 的段落,改名後全稿更新。

## Acceptance criteria

- [ ] migration 建 `speaker_labels(meeting_id, raw_label, display_name, source)`,PK `(meeting_id, raw_label)`;加法式、`IF NOT EXISTS`、冪等、無 version table(對齊既有 migration 慣例)
- [ ] 後端指令 `set_speaker_label(meeting_id, raw_label, display_name)`:upsert 一列,`source='manual'`;註冊進 `generate_handler!`;`models.rs` ↔ `types.ts` 對齊(snake_case)
- [ ] 讀取路徑(latest-run segments + meeting detail)有 label 時以 `display_name` 覆蓋 `speaker`,無 label 保留原值
- [ ] `tauri.ts` COMMANDS + `mockInvoke` case;`mocks.ts` 提供帶 "Speaker 1/2" 的樣本段落
- [ ] 前端:逐字稿 speaker chip 可點 → 輸入名字 → 同 meeting 該 `raw_label` 全段即時更新
- [ ] 測試:resolve-at-read(有/無 label/覆蓋/多群)、`set_speaker_label` upsert 冪等、前端「標一次→全稿更新」(vitest)
- [ ] `npm run build`(tsc + vite)與 `cargo test`(預設 features)全綠

## Blocked by

None - can start immediately.
