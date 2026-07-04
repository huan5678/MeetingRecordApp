# 03 — 出貨 build 開 diarize + 附 sherpa 模型

Status: ready-for-human(鷹架完成;待 Windows build 驗證)
Type: HITL(需先拍板模型散佈方式)

## 決策(已拍板 2026-07-04)

- **模型散佈**:隨 app 打包(tauri resources)。
- **模型**:segmentation = pyannote-segmentation-3.0;embedding = 3D-Speaker **CAM++ zh-cn**(`3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx`)。

## 鷹架(已完成,fetch 已在 Mac 驗過)

- `scripts/fetch-diarize-models.mjs` → 下載+解壓,產出 `src-tauri/resources/models/{sherpa-segmentation,sherpa-embedding}.onnx`(**Mac 實跑驗證通過**:5.7MB + 27MB)。
- `src-tauri/tauri.diarize.conf.json` → 把兩個模型 map 進 bundle resources(放到 `resource_dir()/models/`,對齊 worker 的 `diarize_gemini_segments` 路徑)。
- `.gitignore` 排除模型(不進 git);`package.json` 加 `models:diarize` 與 `build:diarize`。
- 預設 `tauri.conf.json` **不動** → Mac 預設 build 照樣過。

## Windows build 步驟(一鍵)

```
npm run build:diarize
```
= `fetch 模型 → tauri build --features diarize,opencc --config src-tauri/tauri.diarize.conf.json`

**前提**:Windows build 機器要有 **CMake**(sherpa-rs 編 onnxruntime 需要,同 whisper)。若 `--config` 路徑報錯,改成相對 `src-tauri` 的路徑再試。

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

- [x] 決定並記錄模型散佈方式與模型選擇
- [x] fetch script + diarize config + gitignore + npm scripts(fetch 已 Mac 驗)
- [x] 預設(無 feature)build 仍能編、`cargo test` 全綠(不回歸;預設 config 未動)
- [ ] **Windows**:`npm run build:diarize` 成功編出(確認 CMake 到位)
- [ ] **Windows**:app 執行期在 `resource_dir()/models/` 找到兩個模型
- [ ] **Windows**:雙人錄音經 Gemini 轉錄後 `transcript_segments.speaker` 為 "Speaker 1/2"(而非 Gemini 的「講者 N」)→ 逐字稿可對 chip 命名

## Blocked by

None. (為 #02 在 runtime 產出 Speaker N 的前置條件。)
