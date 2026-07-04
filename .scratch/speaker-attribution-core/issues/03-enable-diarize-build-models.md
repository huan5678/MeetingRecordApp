# 03 — 出貨 build 開 diarize + 附 sherpa 模型

Status: ready-for-human
Type: HITL(需先拍板模型散佈方式)

Spec: `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md` §C2 §風險

## What to build

讓出貨的 Windows build 真正啟用 diarization。目前 build 設定裡**沒有** `--features diarize` → `diarize()` 回空 → speaker 全 None,#02 的 C2 在 runtime 空轉。

- build 加 `--features diarize`(或 `full`)
- 綁定 sherpa segmentation + speaker-embedding ONNX 模型
- 把 `DiarizeConfig` 的模型路徑接到 app 執行期找得到的位置(bundle resource 或設定)

## 需人拍板的決策(HITL)

1. **模型散佈**:隨 app 打包(安裝檔變大)vs 首次啟動下載。
2. **模型選擇**:用哪組 sherpa segmentation / embedding 模型(對齊既有中文偏好)。

## Acceptance criteria

- [ ] 決定並記錄模型散佈方式與模型選擇
- [ ] Windows build 以 `diarize`(或 `full`)feature 編出
- [ ] app 執行期能找到 segmentation + embedding 模型,`DiarizeConfig` 正確指向
- [ ] Windows 實跑:雙人錄音轉錄後 `transcript_segments.speaker` 為 "Speaker 1/2"
- [ ] 預設(無 feature)build 仍能編、`cargo test` 全綠(不回歸)

## Blocked by

None. (為 #02 在 runtime 產出 Speaker N 的前置條件。)
