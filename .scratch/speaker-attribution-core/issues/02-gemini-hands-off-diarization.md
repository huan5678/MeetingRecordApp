# 02 — Gemini 交出分段(C1 + C2)

Status: in-progress(C2 碼完成;runtime 需 #03;C1 降為選配)
Type: AFK(碼);runtime 實跑需 #03

Spec: `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md` §C1 §C2

## What to build

把「切分發話人」從 Gemini 收回,交給既有的 sherpa diarization —— 這是拔掉錯名源頭的關鍵。

- **C1**:Gemini transcription prompt 移除「識別並標記發話人(講者 1/2)」指令,只輸出 `{start_ms, end_ms, text}`,`speaker` 一律 null。Gemini 不再兼任 diarizer(它靠上下文猜,常拆錯/併錯人)。
- **C2**:Gemini 轉錄路徑在寫 DB 前,對 segments 跑既有 `Diarizer::diarize` + `assign_speakers` 填 "Speaker N"(whisper 路徑本就如此)。無 `diarize` feature 時 `diarize()` 回空 → speaker 全 None,優雅降級、零回歸。

C1 與 C2 **一起出**,避免「拿掉講者 N 又還沒填 Speaker N」的空窗。

## Acceptance criteria

- [ ] Gemini prompt(單段 chunk + 整體兩處)移除發話人識別指令;回應 schema 不再要求 speaker,或解析後恆為 null
- [ ] `gemini_audio` 既有測試更新:斷言解析後 `segment.speaker` 恆 `None`(移除「講者 1」斷言)
- [ ] worker Gemini 路徑:寫 DB 前跑 `diarize` + `assign_speakers` 填 `speaker`
- [ ] 預設(無 `diarize` feature)`cargo test` 全綠、speaker 皆 None(不回歸)
- [ ] (Windows 手測,#03 之後)真雙人錄音 → `transcript_segments.speaker` 為 "Speaker 1/2",非 Gemini 亂猜

## Blocked by

None for code. Runtime verification blocked by #03(diarize feature 需開啟才會真的填 Speaker N)。

## Comments

**C2 done, design refined (2026-07-04, `ff3f09d`).** diarization 改成 **override 而非 replace**:`apply_turns_to_segments` 只在 turn 覆蓋該段時把 speaker 覆寫成 "Speaker N",沒覆蓋就**保留 Gemini 的 "講者 N"**。
- 好處:diarize 關(預設/#03 前)→ Gemini 標籤照舊,#01 隨時有 chip 可點,**零空窗、零回歸**;diarize 開 → 真 "Speaker N"。
- **因此 C1(拿掉 Gemini prompt 的發話人指令)不再是必要**,降為「省 token 的選配」,#03 在 Windows 驗過 diarization 真的會填之後再做(或不做)。
- 純核心 `apply_turns_to_segments` 有單元測試(Mac 綠);gated 的 `diarize_gemini_segments` 接線只能靠邏輯 + Windows 驗(CLAUDE.md 慣例)。
