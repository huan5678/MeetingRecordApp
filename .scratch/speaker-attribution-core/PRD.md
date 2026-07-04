# Feature: 發話人歸屬核心(Phase 1)

**Spec:** `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md`

把「切分發話人」從 Gemini 收回,交給既有的 sherpa diarization;Gemini 只出文字。真名由 `speaker_labels` 匯流表填 —— 先做「手動標一次」,identity provider / 聲紋記憶列後續 phase。

## Phase 1 切片(tracer bullets)

| # | 切片 | 型態 | Blocked by |
|---|---|---|---|
| 01 | 命名層端到端(`speaker_labels` + 標一次 UI) | AFK | 無 |
| 02 | Gemini 交出分段(C1+C2) | AFK 碼 / Windows 手測 | 碼無;runtime 需 03 |
| 03 | 出貨 build 開 `diarize` + 附 sherpa 模型 | HITL | 無 |

合體:03 打開 diarization → 02 填「Speaker N」→ 01 標一次變真名。

後續 phase(不在此):Teams UIA / Meet caption provider、聲紋記憶、會議偵測觸發。
