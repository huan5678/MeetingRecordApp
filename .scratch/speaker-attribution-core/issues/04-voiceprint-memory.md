# 04 — 聲紋記憶(跨會議自動命名)

Status: in-progress(04a–04c 碼完成 + Mac 測綠;04d 選配跳過;runtime 需 Windows diarize build 驗)
Type: AFK(碼可 Mac 測)/ Windows 手測(gated 抽取 + 準度調校)

Spec: `docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md` §C3 §分期 Phase 3

## What to build

讓「標一次」變「記一輩子」。使用者在某場會標「Speaker 3 → 王經理」後,記住這個人的**聲紋**;下次會議 diarization 又切出同一個聲音,就自動把 `speaker_labels` 預填成「王經理」(`source='voiceprint'`),使用者不用再標。

**為什麼值得做**(對齊使用者情境):

- attendee-first + 沒行事曆 → 沒有帳號名可對。聲紋是唯一「跨會議認得同一個人」的訊號。
- **會議室多人麥克風**:Teams 上是單一帳號,但 diarization 會把現場拆成 Speaker 1/2/3;聲紋記憶讓「現場那幾個常出現的人」下次自動帶名,這是帳號名做不到的。

## 可行性(已確認 sherpa-rs 0.6.8)

- 抽聲紋:`speaker_id::EmbeddingExtractor::compute_speaker_embedding(samples: Vec<f32>, sample_rate: u32) -> Result<Vec<f32>>`(`ExtractorConfig { model, provider, num_threads, debug }`,有 `embedding_size`)。
- 用的就是**已打包的 `sherpa-embedding.onnx`(3D-Speaker CAM++)** —— 不用多帶模型、不動 build。全在既有 `diarize` feature 下(Windows build 已開)。
- 比對**不用** sherpa 的 `EmbeddingManager`:自己寫 ~8 行 cosine + argmax + 門檻(純函式、可 Mac 測、免 gating)。只有「抽取」需要 sherpa(gated)。

## 資料模型(migration `004_speaker_voiceprints.sql`,加法式冪等、無 version table)

```sql
-- 轉錄時算好的「每場每群」聲紋(命名前的暫存,供之後登錄用)
CREATE TABLE IF NOT EXISTS meeting_cluster_embeddings (
    meeting_id TEXT NOT NULL,
    raw_label  TEXT NOT NULL,      -- "Speaker 1" 等 diarization 原始標籤
    embedding  BLOB NOT NULL,      -- f32 little-endian,dim 個
    dim        INTEGER NOT NULL,
    PRIMARY KEY (meeting_id, raw_label)
);
-- 已命名的聲紋庫(跨會議記憶本體)
CREATE TABLE IF NOT EXISTS speaker_voiceprints (
    name       TEXT NOT NULL PRIMARY KEY,   -- ponytail: 一名一紋,最新登錄覆蓋;多樣本平均待命中率不佳再說
    embedding  BLOB NOT NULL,
    dim        INTEGER NOT NULL
);
```

BLOB 編碼:`f32` 直接 `to_le_bytes` 串接;解碼 `chunks_exact(4)` → `f32::from_le_bytes`。`dim` 另存供驗證。**不加新依賴**(不用 bytemuck)。

## 切片(tracer bullets)

| # | 切片 | 型態 | 說明 |
|---|---|---|---|
| 04a | 轉錄時抽 + 存每群聲紋 | 碼(抽取 gated)| worker diarization 後,對每個 cluster 蒐集其 PCM → `compute_speaker_embedding` → 存 `meeting_cluster_embeddings`。切 PCH 的 `cluster_pcm_for(turns, pcm, speaker_id)` 純函式可 Mac 測;sherpa 抽取 gated。 |
| 04b | 命名時登錄聲紋 | 碼(純 SQL,Mac 測)| 擴充 `set_speaker_label`:`source='manual'` 時,把 `meeting_cluster_embeddings(meeting_id, raw_label)` 那列複製進 `speaker_voiceprints(display_name)`(upsert)。純 SQL,塞假 blob 即可測。 |
| 04c | 轉錄時自動比對 pre-fill | 碼(比對純函式 Mac 測)| 算完每群聲紋後,對 `speaker_voiceprints` 全庫做 cosine,最佳分 ≥ 門檻 → upsert `speaker_labels(meeting_id, raw_label, name, source='voiceprint')`。cosine/argmax/門檻純函式可測。 |
| 04d | 門檻旋鈕 + 來源標示 | 選配 / Windows | `VOICEPRINT_MATCH_THRESHOLD: f32 = 0.5`(sherpa 預設)加 `ponytail:` 註解;逐字稿對 `source='voiceprint'` 的 chip 標「聲紋」小記號,讓使用者知道是猜的、可改。準度靠 Windows 實測調門檻。 |

**串接**:04a 產生每群聲紋 → 04c 拿它比對舊庫自動命名 → 使用者若不滿意用 #01 手改 → 04b 把手改的結果登錄回庫(下次更準)。閉環。

## 降級 / 零回歸

- 無 `diarize` feature / 非 Windows:04a 的抽取是 no-op(gated),兩張表空 → 04b/04c 無事可做 → 行為同今日。#01 手動命名照常。
- 聲紋沒命中:留「Speaker N」待手動(同 Phase 1)。
- `source='voiceprint'` 的預填**可被使用者覆寫**(#01 的 rename 以 raw_label 為 key,照樣蓋過)。

## Acceptance criteria

- [x] migration `004`:兩表建立,加法式、`IF NOT EXISTS`、冪等、無 version table;migration 建表測試涵蓋
- [x] BLOB 編解碼 round-trip 純函式測(`Vec<f32>` ↔ bytes)
- [x] 04a:`cluster_pcm_for` 純函式測(依 turns 時間段切對 PCM,含越界 clamp);gated 抽取接線(`voiceprint_clusters`,邏輯 + Windows 驗)
- [x] 04b:手動命名後 `enroll_voiceprint_from_cluster` 把該群聲紋登錄進 `speaker_voiceprints`(Rust roundtrip 測,含 upsert / 無群 no-op);`set_speaker_label` 指令接線
- [x] 04c:`best_voiceprint_match`(cosine + argmax + 門檻)純函式測(命中/未命中/多庫最佳/空庫);`prefill_speaker_label_from_voiceprint` 用 `DO NOTHING` 不蓋手動標籤(測)
- [x] 預設(無 `diarize`)`cargo test` 全綠(265)、兩表空、零回歸;前端 43 綠
- [ ] (Windows 手測)A 場標「Speaker 1 → 某人」→ B 場同人自動預填該名;門檻可調
- [~] 04d 來源標示 chip(選配):**跳過**。`source` 已存 DB,但 `get_speaker_labels` 只回 `{raw→display}`;加徽章要改 IPC 回傳形狀 + 前端,列後續按需再做。

## 天花板 / 未知(給 `ponytail:` 註解)

- **一名一紋**:多樣本平均 / 多列比對留給命中率不佳時再上(`// ponytail: single voiceprint per name; multi-sample averaging if match rate poor`)。
- **門檻 0.5** 是 sherpa 預設;會議室遠場 / 搶話會讓聲紋劣化,**準度只能 Windows 實測調**。
- 比對用**單一最佳分**(argmax);若出現誤命中,升級成「最佳與次佳需拉開 margin」再判。
- 短講群(某人只講一兩句)聲紋不穩 → 可設 `MIN_CLUSTER_MS` 才登錄/比對(選配)。

## Blocked by

Phase 1(#01 命名層 + #02 diarization 權威 + #03 Windows diarize build)。核心碼可先寫可先測,但**真正有價值要 #03 的 diarize build 在 Windows 跑起來**才驗得到。建議 #01–#03 Windows 實測過、diarization 分群準度確認堪用後再開工。
