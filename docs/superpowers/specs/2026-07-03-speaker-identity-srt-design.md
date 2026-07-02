# 設計:發話人身分流 SRT + overlap-join

**日期:** 2026-07-03
**狀態:** 待審
**範圍:** v1.0 Windows-only,audio-only。非 Windows / 非 Teams 全部優雅降級為 no-op。

## 問題

App 錄的是 Windows loopback 混音,**沒有身分**。要讓逐字稿/摘要的人名對到真人,需要一條**帶時間戳、帶帳號名稱**的獨立訊號,再跟逐字稿用時間對齊。

## 第一性原理拆解

兩條互相獨立的資料流:

- **身分流(誰,何時)** — 錄音當下輪詢 Teams 的「當前發話人」名稱 → `(name, start_ms, end_ms)` 時間段。**不需要逐字稿,也不需要 diarization。** 零 ML、零 API 成本。落地成一份 `speakers.srt` sidecar。
- **文字流(說什麼,何時)** — 事後由 Gemini/whisper 產生,已有 `TranscriptSegment { start_time_ms, end_time_ms }`。

事後用**機械 overlap-join**把身分流的名字填進 `transcript_segments.speaker`(目前放 `Speaker N` 或 `None`)。LLM 拿到的逐字稿本來就標好真名。

## Non-goals(刻意跳過)

- 逐句「保證」對到 AAD 帳號 → 那要 Graph 媒體 bot(另案)。本設計拿到的是 Teams UI 顯示名稱。
- 搶話/同時發言的完整分離 → active-speaker 只給**主要**發話人,crosstalk 會塌成一人。可接受。
- 即時(錄音中)顯示對應 → 對齊是事後批次做。
- 聲紋註冊、行事曆邀請名單。

## 架構

五個單元,各一職責。前四個是可獨立測試的純邏輯 / 平台隔離,第五個是接線。

```
錄音開始 ──┬─> 音訊寫 WAV (既有)
           └─> [U1] Speaker poller (Windows UIA, N Hz)
                     │ 送出 (t_ms, Option<name>) 樣本
                     v
               [U2] Span builder (純, debounce)
                     │ (name, start_ms, end_ms) 時間段
                     v
錄音結束 ──> [U3] 寫 speakers.srt sidecar (重用 export::fmt_timestamp)

轉錄完成 (worker.rs 產生 segments) ──>
               [U4] overlap-join (純) : segments × spans -> 填 speaker
                     v
               [U5] 接線:寫回 DB segments + 名單當作 LLM 提示
```

### U1 — Speaker poller(平台隔離)

新檔 `src-tauri/src/detection/speaker.rs`,**照抄 `detection/monitor.rs` 既有模式**(它已經在做 `GetForegroundWindow` + `GetWindowTextW` 的 Windows 輪詢 + 非 Windows 回 `unsupported_platform`)。

- `#[cfg(windows)]`:用 Windows UI Automation(`uiautomation` crate)讀 Teams 主舞台「當前發話人」名稱標籤。每 `POLL_MS` 取一次樣本 `(elapsed_ms, Option<String>)`。讀不到 → `None` 樣本。
- 非 Windows:no-op,永不產生樣本。
- 只在 `MeetingApp::Teams`(既有 enum)是前景會議 app 時才嘗試;否則送 `None`。
- 生命週期綁錄音 session:session 開始 spawn thread,結束時停止並回收樣本。

**時鐘:** session 開始時抓一個單調 `Instant` 當基準;`elapsed_ms = now - start`。與音訊/逐字稿的 offset **同一基準**(既有 SRT export 就是 offset-based),對齊才不飄。

### U2 — Span builder(純,可測)

`fn build_spans(samples: &[(u64, Option<String>)]) -> Vec<SpeakerSpan>`

- 合併連續相同 name 的樣本成一段 `[first_t, last_t + POLL_MS]`。
- `None` 樣本 → 關閉當前段。
- 樣本間隔 > `MAX_GAP_MS`(漏抓)→ 在 `last_t + POLL_MS` 關段。
- 丟棄短於 `MIN_SPAN_MS` 的段(去 flicker)。

```rust
struct SpeakerSpan { name: String, start_ms: u64, end_ms: u64 }
```

### U3 — Identity SRT sidecar

- 寫 `speakers.srt` 到 meeting 的 FileStore 目錄(跟 WAV、既有 `transcript.md`/`summary.md`/`.srt` sidecar 並列)。cue 內文 = 名字。重用 `export::fmt_timestamp(ms, ',')`。
- 讀回:一個小 parser(或直接用記憶體中的 `Vec<SpeakerSpan>`,錄完馬上 join 就不必 parse)。**sidecar 檔就是 source of truth,不開 DB 表。** 之後重跑轉錄能再讀它做 join。

範例:
```
1
00:00:03,200 --> 00:00:11,750
Alice Chen

2
00:00:11,750 --> 00:00:15,000
Bob Wang
```

### U4 — overlap-join(純,可測,核心正確性)

`fn assign_speakers(segments: &mut [TranscriptSegment], spans: &[SpeakerSpan])`

對每個 segment `[s, e]`:
1. 對每個 span 算重疊毫秒 `overlap = max(0, min(e, span.end) - max(s, span.start))`。
2. 取 `overlap` 最大的 span。平手 → 取較早的。
3. `best_overlap >= MIN_OVERLAP_FRAC * (e - s)` 才寫 `segment.speaker = Some(name)`;否則**保留既有值**(diarization 的 `Speaker N` 或 `None`)。

跨邊界的 segment 由「多數重疊」決定;沒有身分覆蓋的段落安靜地留給 diarization。**確定性,不用 LLM 賭。**

### U5 — 接線

- **Session 開始**(`audio` / `commands` 起錄處):存單調 start `Instant` + spawn U1。
- **Session 結束:** 停 U1 → `build_spans` → 寫 `speakers.srt`。
- **轉錄完成後**(`transcription/worker.rs`,segments **寫 DB 之前**):若該 meeting 有 `speakers.srt`,在記憶體中的 segments 上跑 U4 填 `speaker`,再一次寫入。重跑轉錄時重讀 sidecar 再 join。
- **LLM 提示:** segments 已帶真名,既有摘要器自動受惠。額外把「出現過的名字集合」當 roster 前置到轉錄/摘要 prompt(即先前 Path C 的做法),幫 LLM 補洞。

## 優雅降級(零回歸)

| 情況 | 結果 |
|---|---|
| 非 Windows | U1 no-op → 無 `speakers.srt` → 跳過 U4 → 同今日行為 |
| 前景非 Teams | 全程送 `None` → 無 span → 跳過 U4 |
| UIA 讀不到發話人 | 同上,優雅降級 |
| 有身分流但某段沒覆蓋 | 該段留 diarization 標籤 / `None` |

## 可調旋鈕(real-world tuning,給常數 + `ponytail:` 註解)

| 常數 | 預設 | 作用 |
|---|---|---|
| `POLL_MS` | 250 (4 Hz) | 取樣率 = 時間解析度上限 |
| `MIN_SPAN_MS` | 600 | 短於此的段視為 flicker 丟棄 |
| `MAX_GAP_MS` | 750 | 樣本間隔超過就關段 |
| `MIN_OVERLAP_FRAC` | 0.5 | segment 要被覆蓋多少比例才貼名字 |

天花板要講清楚:active-speaker 有 ~0.5–1s 延遲、只給主要發話人。要逐字級精準得上 bot。

## Phase 0 — 擋路的唯一未知數(先驗再投資)

**在真的 Windows Teams(新版 WebView2)上驗 UIA 能不能讀到「當前發話人」名稱。** 全部下游都靠這個。

驗證清單:
- [ ] 用 `inspect.exe` / Accessibility Insights 看 Teams 通話視窗的 UIA 樹。
- [ ] 主舞台中央「當前發話人」名稱標籤,是否有可讀的 UIA `Name`?(通常比每格 speaking ring 好讀)
- [ ] 切換發話人時該標籤是否即時更新?延遲多少?
- [ ] 語系不同時控制項識別怎麼變?
- [ ] 若主舞台讀不到 → 退而驗參與者面板 speaking 狀態是否為 UIA property。
- [ ] 全讀不到 → 本設計對 Teams 無效,結案改走 Graph bot。

## 要動的檔案

- 新:`src-tauri/src/detection/speaker.rs`(U1 + U2)。
- 新:`speakers.srt` 寫入 — 重用 `export::fmt_timestamp`,寫進 FileStore meeting 目錄(U3)。
- 新:overlap-join 純函式(U4)—放 `transcription/` 或 `detection/speaker.rs`。
- 改:`transcription/worker.rs` — 轉錄後呼叫 U4(U5)。
- 改:錄音 session 起訖處 — start `Instant` + spawn/stop poller(U5)。
- 改:轉錄/摘要 prompt — 前置 roster 名單(U5,重用 Path C)。
- `Cargo.toml`:加 `uiautomation`(Windows target-gated dependency)。

## 測試

純函式各留一個 `assert` 測試(ponytail:非平凡邏輯才測):
- `build_spans`:合併 + flicker 丟棄 + `None` 關段 + gap 關段。
- `assign_speakers`:乾淨對應、跨邊界多數決、無重疊留空、平手取較早。

平台層(U1 UIA)在 Mac 上 `cargo check` 不到,靠邏輯 + Windows 手測(同 whisper 慣例)。

## 分期

1. **Phase 0** — UIA spike(上面清單)。**gate,先做。**
2. **Phase 1** — U2 + U4 純函式 + 測試(不碰平台,Mac 上可全綠)。
3. **Phase 2** — U1 poller + U3 sidecar + U5 接線,Windows 手測。
4. **Phase 3**(選配)— roster 前置 prompt 補洞。

## 實作狀態(2026-07-03)

TDD 完成、`cargo test` 全綠(251 lib + 1 E2E),`src-tauri/src/detection/speaker.rs`:

- ✅ **U2 `build_spans`** — 純 debounce,2 測試(merge/flicker/None-close、gap-split)。
- ✅ **U3 `to_speaker_srt` / `parse_speaker_srt`** — 重用 `export::fmt_timestamp`,round-trip + 容錯測試。
- ✅ **U4 `assign_speakers`** — overlap-join,涵蓋乾淨對應/跨邊界多數決/平手/低於門檻保留既有。
- ✅ **U1 `SpeakerMonitor` / `SpeakerCapture`** — 鏡射 `detection/monitor.rs`,單調時鐘 + stop flag,lifecycle 測試(非 Windows path)。**`read_active_speaker()` 目前回 `None`(Phase 0 gate)** — UIA 讀取待 spike 驗證後補上,是唯一未完成的一函式。
- ✅ **U5 接線** — `commands.rs` 錄音起 spawn poller / 停止 join+寫 `speakers.srt`(spans 非空才寫);`transcription/worker.rs` 兩條路徑(Gemini + whisper)在寫 DB 前 `apply_speaker_identity` 做 overlap-join。全程優雅降級,零回歸。
- ✅ **E2E(Mac-runnable)** — `src-tauri/tests/speaker_pipeline_e2e.rs`:samples → build_spans → speakers.srt 文字 → parse → overlap-join → 具名逐字稿,確定性斷言。

### 尚待(需 Windows / 需真人會議,本機無法跑)

- **Phase 0 UIA spike**(見上方清單)→ 填 `read_active_speaker()`。
- **裝置級 E2E(Windows 手測)**:
  1. Windows 上 `git fetch origin; git reset --hard origin/main`,`npm run tauri build`。
  2. 開 Teams 通話,本 app 開始錄音 → 說話切換數位發話人。
  3. 停止錄音 → 檢查 meeting 目錄下有 `speakers.srt` 且名字/時間段合理。
  4. 轉錄完成 → 逐字稿 `speaker` 欄位為真名(非 `Speaker N`),摘要人名正確。
- **Phase 3**(選配)roster 前置 prompt。
