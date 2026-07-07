# 設計:發話人歸屬核心 — diarization 擁有分段 + 標一次命名 + 可插拔身分來源

**日期:** 2026-07-04
**狀態:** 待審
**範圍:** v1.0 Windows-only、audio-only。承接並**重新定位** 2026-07-03「發話人身分流 SRT」spec —— 該案的 Teams UIA 身分流在此降級為「其中一個**可選的命名 provider**」,不再是命脈。

## 問題

現況:Gemini transcription prompt(`ai/gemini_audio.rs` 的 `build_transcript_prompt` / `build_prompt`)同時要求它「切分發話人並標成『講者 1/2』」(指令 2)。等於**讓 Gemini 兼任 diarizer**。Gemini 不是聲學 diarizer,靠上下文猜發話人切換,常把同一人拆成兩人、或把兩人併一人 → 「講者 N」不穩、對不到真人 → 使用者回報的「沒法有效對應使用者名稱、人名錯誤」。

三個約束(使用者已定):

- **attendee-first**:使用者多半是「加入別人會議的參與者」,不是主辦者。
- **沒有行事曆**:多半直接收連結;就算有,受邀者 ≠ 實際與會者。
- 因此:平台雲端 API(Teams Graph / Zoom RTMS / Meet API)幾乎都要主辦者/租戶權限 → 不可用;連 Notion 抄的「行事曆出席者命名」也用不上。

三輪 deep-research 結論(2026-07-04):

- 純 attendee 端「讀會議 UI 拿真名」只有 **Google Meet(讀自家 caption DOM)** 有現成開源先例;**Teams active-speaker 的 UIA 讀取未經證實**(名字多半畫在 GPU 視訊 tile、不在 a11y 樹;roster 讀得到,但「誰正在講」沒把握);Zoom 無先例。
- **所有同棧競品**(TalkTrack、OpenWhispr —— 同為 WASAPI loopback + sherpa-onnx)都不是讀 UI,而是**聲學 diarization + 手動/行事曆命名**。這就是務實解。

## 第一性原理:把三件事解耦

| 問題 | 誰負責(改後) | 現況 |
|---|---|---|
| 誰在何時說(**分段**) | **本地 diarization(sherpa-onnx)** | 已有 `transcription/diarization.rs`,但目前被 Gemini 的猜測蓋過 |
| 說了什麼(**文字**) | **Gemini,只出文字、不猜發話人** | prompt 目前還要求它標發話人 → 拿掉 |
| 哪個真名(**命名**) | **一張 label 匯流表,多來源填** | 尚無 |

命名的來源(全部 upsert 進同一張 `speaker_labels`):

1. **手動標一次(核心)** — 使用者替「Speaker 1」命名一次 → 該 meeting 全部該群更新。
2. **身分 provider(可選,自動 pre-fill)** — Teams UIA 身分流(既有,spike-gated)、Meet caption 擴充(後續)。
3. **聲紋記憶(後續 Phase 3)** — 持久化每群 embedding + 名字,下次自動命名。

## 為什麼這樣(研究支撐)

- 不賭 Gemini 猜名、不賭 Teams UIA(未證實)。用**已建好**的 diarization 當骨幹。
- 與同棧競品一致(TalkTrack / OpenWhispr),務實、跨平台、今天就能做。
- 既有的 identity-SRT(U1–U5)**不浪費**:它自然變成「provider 1」—— 當 UIA spike 過關,它自動 pre-fill label 表;沒過關,核心照樣運作。

## 架構

**重用(不動):**

- `transcription/diarization.rs`:`Diarizer::diarize` → `SpeakerTurn`;`assign_speakers` → `segment.speaker = "Speaker N"`。純函式已測。
- `detection/speaker.rs`(identity-SRT U1–U5)、`export::fmt_timestamp`、overlap-join。
- summarizer 已把 `speaker: text` 攤平 → 一旦 speaker 是真名,摘要自動受惠。

### C1 — Gemini 只出文字(核心;最小改動、最大效果)

`ai/gemini_audio.rs`:`build_transcript_prompt` / `build_prompt` **移除指令**「2) Identify distinct speakers and label…」。改為只輸出 `{start_ms, end_ms, text}`,`speaker` 一律 null。**發話人分段不再由 Gemini 猜。**

### C2 — diarization 成為 speaker 權威

`transcription/worker.rs`:Gemini 路徑在寫 DB 前,對 segments 跑 `Diarizer::diarize` + `assign_speakers` 填「Speaker N」(whisper 路徑本來就這樣)。

- 依賴:sherpa 原生只在 `diarize` feature 下編。**出貨的 Windows build 必須開 `--features diarize`(或 full)**;否則 `diarize()` 回空 → speaker 全 None(優雅降級,不回歸)。
- 需 diarization 模型檔(sherpa segmentation + embedding ONNX)隨附/設定(既有 `DiarizeConfig`)。

### C3 — 命名層:`speaker_labels` + resolve-at-read + 標一次 UI(核心新 UX)

- 新表 `speaker_labels(meeting_id TEXT, raw_label TEXT, display_name TEXT, source TEXT, PRIMARY KEY(meeting_id, raw_label))`。加法式、`CREATE … IF NOT EXISTS`、冪等(對齊 `database.rs` 無 version-table 慣例)。
- **解析放前端**(實作定案):segments 一律保留**原始** `speaker`(`"Speaker N"`),不在後端 overlay 覆寫;前端用 `get_speaker_labels(meeting_id)` 拿 `{raw → display}`,渲染時 `labels[raw] ?? raw`。這樣 rename 永遠以**原始標籤**為 key,顯示名可無限次重改(overlay-at-read 會把 `speaker` 換成顯示名 → 二次改名找不到原始 key)。export/summary 拿真名列後續(在各自出口點解析)。
- 新指令 `set_speaker_label(meeting_id, raw_label, display_name)` + `get_speaker_labels(meeting_id)`(+ `mockInvoke` 有狀態 store)。前端:逐字稿 speaker chip 可點 → 輸入名字 → `onRelabel` 重載 labels → 全稿該群即時更新。
- roster 前置 prompt(既有 Path C / identity-SRT U5 Phase 3):把已知名字集合前置到**摘要** prompt,補洞。

### C4 — 身分 provider(可選,自動 pre-fill C3)

- **Teams UIA(既有,spike-gated)**:identity-SRT 產 `speakers.srt`;overlap-join 後,把「Speaker N → 真名」以 `source='teams-uia'` upsert 進 `speaker_labels`(而非直接覆寫 segment)。07-03 的 Phase 0 UIA spike 過才有值。
- **Meet caption 擴充(後續 feature)**:瀏覽器擴充讀 caption(名字+文字+時間)→ 經 local endpoint 灌回 → upsert labels。**架構代價:多一個瀏覽器元件**,破壞「單一桌面 app」UX,列後續再權衡。
- **Zoom**:延後(無先例)。
- **主辦者時**:才回頭用平台 API(Teams Graph 等,機會財,另案)。

## Non-goals(刻意跳過)

- 逐句保證對到 AAD 帳號(要 Graph 媒體 bot)。
- crosstalk / 搶話完整分離(diarization 以主要發話人為主)。
- 即時(錄音中)命名 —— 命名是事後批次。
- 聲紋記憶列 Phase 3,非本核心。

## 優雅降級(零回歸)

| 情況 | 結果 |
|---|---|
| 無 `diarize` feature / 非 Windows | speaker 全 None(同今日 Gemini-null 段),照出文字稿 |
| 有 diarization、無命名 | 顯示「Speaker 1/2」,使用者可標一次 |
| 有身分 provider | 自動 pre-fill 真名,使用者可改 |
| 某群沒被 provider 覆蓋 | 留「Speaker N」待手動 |

## 資料模型

`speaker_labels` 新表(見 C3)。migration 加法式冪等、無 version table(對齊 `storage/database.rs`)。`types.ts` ↔ `models.rs` 對應(snake_case,對齊 `tauri.ts` IPC 契約:COMMANDS / SETTINGS_KEYS / mock)。

## 可調旋鈕(給常數 + `ponytail:` 註解)

- 沿用 identity-SRT 既有:`POLL_MS` / `MIN_SPAN_MS` / `MAX_GAP_MS` / `MIN_OVERLAP_FRAC`。
- diarization:`DiarizeConfig.num_speakers`(None = 自動估;人數已知時固定更準)。
- 天花板:diarization 對中文會議的分群準度需 Windows 實測調校;active-speaker 有 ~0.5–1s 延遲、只給主要發話人。

## 分期

1. **Phase 1(核心,平台無關,先做)**:C1(Gemini text-only)+ C2(diarization 權威;Windows build 開 diarize)+ C3(`speaker_labels` + resolve + 標一次 UI)。Mac 上純函式 / DB / 前端可全綠;diarization 實跑靠 Windows。
2. **Phase 2(providers,選配)**:完成 07-03 的 Phase 0 UIA spike → 過則接 Teams UIA provider(upsert labels)。Meet caption 擴充。
3. **Phase 3**:聲紋記憶(從 sherpa 取每群 embedding 持久化 + cosine 匹配 + 門檻調校)。**可行性已確認**,見 issue #04。

## 要動的檔案

- 改 `ai/gemini_audio.rs`(C1 prompt;移除 speaker 指令 + 更新測試)。
- 改 `transcription/worker.rs`(C2:Gemini 路徑接 `diarize` + `assign_speakers`)。
- 新 `migrations/*.sql`(`speaker_labels`)。
- 改 `storage/database.rs`(建表 + resolve-at-read join)、`commands.rs`(`set_speaker_label` + 註冊)、`lib.rs`(`generate_handler!`)、`models.rs`。
- 改 `src/lib/tauri.ts`(COMMANDS + `mockInvoke`)、`src/lib/types.ts`、逐字稿 UI(speaker chip 可命名)、`src/lib/mocks.ts`。
- 出貨設定:Windows build 開 `--features diarize`(或 full)+ 附 sherpa 模型檔。

## 測試

- `assign_speakers`(既有,綠)。
- resolve-at-read:有 / 無 label、覆蓋、多群(DB 層或純函式各一 `assert`)。
- `set_speaker_label` upsert 冪等。
- Gemini prompt 測試更新:斷言輸出 `speaker` 恆 null(移除既有「講者 1」斷言)。
- 前端:標一次 → 全稿更新(既有 vitest 模式)。
- diarization 實跑 + provider:Windows 手測(同 whisper / UIA 慣例)。

## 風險 / 未知

- **`diarize` feature 必須在 Windows build 開**,否則核心無 speaker(只有 None)。要確認出貨腳本 + 模型檔到位。
- 中文 diarization 品質(sherpa segmentation / embedding 模型對中文會議的分群準度)需 Windows 實測調 `num_speakers` / 門檻。
- 聲紋記憶(Phase 3)取決於 sherpa-rs 是否暴露 embedding API —— **已證實可行**(0.6.8 `EmbeddingExtractor::compute_speaker_embedding` 抽每群聲紋,複用已打包的 3D-Speaker 模型;比對自寫 cosine)。準度(尤其會議室遠場)仍需 Windows 實測。見 issue #04。
- Meet provider 需瀏覽器擴充,列後續再權衡是否值得破壞單一 app UX。
