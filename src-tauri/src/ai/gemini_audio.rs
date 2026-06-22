//! Gemini **multimodal** path: send the meeting WAV directly to Gemini and get
//! back, in ONE `generateContent` call, a transcript (speaker-labelled segments
//! with millisecond timestamps) **and** a summary — in Traditional Chinese.
//!
//! This is the "Gemini-primary" transcription engine. It needs **no native
//! deps** (just `reqwest` + the keychain + `serde_json`), so it compiles and is
//! testable in the default build — unlike the whisper path. Audio is uploaded
//! via the **Files API** (resumable upload), which has no 20 MB inline-request
//! limit and needs no base64, then referenced by `fileUri` in the request.
//!
//! Privacy: the recording is uploaded to Google's cloud (opt-in; requires a
//! Gemini API key). Uploaded files are retained by Google ~48h.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::ai::keychain;
use crate::ai::provider::{AiError, Result, SummaryDraft, SummaryTemplate};
use crate::models::{ActionItem, AiProviderKind, KeyDecision, TranscriptSegment};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const UPLOAD_URL: &str = "https://generativelanguage.googleapis.com/upload/v1beta/files";

/// Default multimodal model (audio-capable, fast tier).
pub const DEFAULT_MODEL: &str = "gemini-3.5-flash";

/// What one Gemini audio call yields: transcript rows + an auto-summary.
pub struct GeminiAudioResult {
    pub segments: Vec<TranscriptSegment>,
    pub summary: SummaryDraft,
    pub language: Option<String>,
}

/// Upload `wav_path` to Gemini and ask for transcript + summary in one call.
pub async fn transcribe_and_summarize(
    wav_path: &Path,
    meeting_id: &str,
    created_at: &str,
    template: SummaryTemplate,
    model: &str,
) -> Result<GeminiAudioResult> {
    let api_key = keychain::get_api_key(AiProviderKind::Gemini)
        .map_err(|e| AiError::Parse(format!("keychain error: {e}")))?
        .ok_or(AiError::MissingApiKey("gemini"))?;

    let client = reqwest::Client::new();
    let bytes =
        std::fs::read(wav_path).map_err(|e| AiError::Parse(format!("read wav: {e}")))?;

    // Recordings are wav; imported files may be mp3/m4a/etc. Gemini decodes by
    // the declared MIME type, so derive it from the extension.
    let mime = mime_for(wav_path);
    let file_uri = upload_audio(&client, &api_key, bytes, mime).await?;

    let prompt = build_prompt(template);
    let body = GenerateContentRequest {
        contents: vec![GContent {
            role: "user",
            parts: vec![
                GPart::File {
                    file_data: GFileData {
                        mime_type: mime,
                        file_uri,
                    },
                },
                GPart::Text { text: prompt },
            ],
        }],
        generation_config: GGenConfig {
            response_mime_type: "application/json",
        },
    };

    let url = format!("{API_BASE}/models/{model}:generateContent");
    let resp = client
        .post(&url)
        .header("x-goog-api-key", &api_key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AiError::Status {
            status: status.as_u16(),
            body: text.chars().take(512).collect(),
        });
    }

    let parsed: GenerateContentResponse =
        resp.json().await.map_err(|e| AiError::Parse(e.to_string()))?;
    let tokens = parsed.usage_metadata.and_then(|u| u.total_token_count);
    let text = parsed
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content)
        .map(|c| {
            c.parts
                .into_iter()
                .filter_map(|p| p.text)
                .collect::<Vec<_>>()
                .join("")
        })
        .ok_or_else(|| AiError::Parse("no candidates in Gemini response".into()))?;

    parse_audio_response(&text, meeting_id, created_at, model, tokens)
}

/// Resumable Files-API upload (no multipart/base64 needed): start → upload bytes
/// → poll until the file is `ACTIVE`. Returns the file's `uri` for `fileUri`.
async fn upload_audio(
    client: &reqwest::Client,
    api_key: &str,
    bytes: Vec<u8>,
    mime: &str,
) -> Result<String> {
    let num_bytes = bytes.len();

    let start = client
        .post(UPLOAD_URL)
        .header("x-goog-api-key", api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", num_bytes.to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime)
        .json(&serde_json::json!({ "file": { "display_name": "meeting" } }))
        .send()
        .await?;
    if !start.status().is_success() {
        let s = start.status().as_u16();
        let b = start.text().await.unwrap_or_default();
        return Err(AiError::Status {
            status: s,
            body: b.chars().take(512).collect(),
        });
    }
    let upload_url = start
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| AiError::Parse("Gemini Files API returned no upload URL".into()))?;

    let finalize = client
        .post(&upload_url)
        .header("Content-Length", num_bytes.to_string())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(bytes)
        .send()
        .await?;
    if !finalize.status().is_success() {
        let s = finalize.status().as_u16();
        let b = finalize.text().await.unwrap_or_default();
        return Err(AiError::Status {
            status: s,
            body: b.chars().take(512).collect(),
        });
    }
    let uploaded: UploadResponse =
        finalize.json().await.map_err(|e| AiError::Parse(e.to_string()))?;
    let file = uploaded.file;

    // Audio is usually ACTIVE immediately, but poll briefly to be safe — a
    // generateContent on a still-PROCESSING file errors.
    if file.state.as_deref() != Some("ACTIVE") {
        if let Some(name) = &file.name {
            for _ in 0..15 {
                tokio::time::sleep(Duration::from_millis(800)).await;
                let g = client
                    .get(format!("{API_BASE}/{name}"))
                    .header("x-goog-api-key", api_key)
                    .send()
                    .await?;
                if let Ok(f) = g.json::<FileObject>().await {
                    match f.state.as_deref() {
                        Some("ACTIVE") => break,
                        Some("FAILED") => {
                            return Err(AiError::Parse("Gemini file processing FAILED".into()))
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(file.uri)
}

/// Parse Gemini's JSON answer into transcript rows + a summary draft. Pure +
/// unit-tested (no network).
fn parse_audio_response(
    text: &str,
    meeting_id: &str,
    created_at: &str,
    model: &str,
    tokens_used: Option<i64>,
) -> Result<GeminiAudioResult> {
    let trimmed = text.trim().trim_start_matches("```json").trim_matches('`').trim();
    let raw: AudioJson = serde_json::from_str(trimmed).map_err(|e| {
        AiError::Parse(format!(
            "Gemini audio JSON parse: {e} (got: {})",
            trimmed.chars().take(200).collect::<String>()
        ))
    })?;

    let language = raw.language.clone();
    let segments = raw
        .segments
        .into_iter()
        .enumerate()
        .map(|(i, s)| TranscriptSegment {
            id: uuid::Uuid::new_v4().to_string(),
            meeting_id: meeting_id.to_string(),
            segment_index: i as i64,
            start_time_ms: s.start_ms,
            end_time_ms: s.end_ms,
            text: s.text.trim().to_string(),
            speaker: s.speaker.filter(|sp| !sp.trim().is_empty()),
            confidence: None,
            language: language.clone(),
            created_at: created_at.to_string(),
        })
        .collect();

    let summary = SummaryDraft {
        content: raw.summary.content.trim().to_string(),
        action_items: raw.summary.action_items,
        key_decisions: raw.summary.key_decisions,
        provider: AiProviderKind::Gemini,
        model: model.to_string(),
        tokens_used,
    };

    Ok(GeminiAudioResult {
        segments,
        summary,
        language,
    })
}

/// Map a file extension to the audio MIME type Gemini understands. Covers the
/// common meeting formats; unknown/extension-less files default to wav (the
/// app's own recording format).
/// ponytail: known formats only; add more rows if users import exotic codecs.
fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") => "audio/mp3",
        Some("m4a") | Some("mp4") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("ogg") | Some("oga") | Some("opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("aiff") | Some("aif") => "audio/aiff",
        Some("webm") => "audio/webm",
        _ => "audio/wav",
    }
}

/// Build the instruction prompt: Traditional Chinese, diarization + ms
/// timestamps, plus the template-specific summary focus and the JSON contract.
fn build_prompt(template: SummaryTemplate) -> String {
    format!(
        "You are a meeting transcription + summarization assistant. The attached audio is a meeting recording.\n\
         1) Transcribe it VERBATIM in Traditional Chinese (zh-TW, 繁體中文). Do NOT output Simplified characters.\n\
         2) Identify distinct speakers and label each segment (e.g. \"講者 1\", \"講者 2\"); use null if unknown.\n\
         3) Give millisecond start/end timestamps for each segment.\n\
         4) Then summarize. {instructions}\n\
         Respond with a SINGLE JSON object and nothing else (no prose, no code fence), with exactly these keys:\n\
         \"language\": string (e.g. \"zh-TW\"),\n\
         \"segments\": array of {{\"start_ms\": integer, \"end_ms\": integer, \"speaker\": string|null, \"text\": string}},\n\
         \"summary\": {{\"content\": a Traditional-Chinese Markdown summary, \"action_items\": array of {{\"task\": string, \"owner\": string|null, \"deadline\": string|null}}, \"key_decisions\": array of {{\"decision\": string, \"context\": string|null}}}}.\n\
         Use null (not empty string) for unknown owner/deadline/context; use empty arrays when there are none.",
        instructions = template.instructions(),
    )
}

// ---- request / response wire types ----------------------------------------

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<GContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GGenConfig,
}

#[derive(Serialize)]
struct GContent {
    role: &'static str,
    parts: Vec<GPart>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum GPart {
    Text {
        text: String,
    },
    File {
        #[serde(rename = "fileData")]
        file_data: GFileData,
    },
}

#[derive(Serialize)]
struct GFileData {
    #[serde(rename = "mimeType")]
    mime_type: &'static str,
    #[serde(rename = "fileUri")]
    file_uri: String,
}

#[derive(Serialize)]
struct GGenConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: &'static str,
}

#[derive(Deserialize)]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata", default)]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<ResponseContent>,
}

#[derive(Deserialize)]
struct ResponseContent {
    #[serde(default)]
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize)]
struct ResponsePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct UsageMetadata {
    #[serde(rename = "totalTokenCount", default)]
    total_token_count: Option<i64>,
}

#[derive(Deserialize)]
struct UploadResponse {
    file: FileObject,
}

#[derive(Deserialize)]
struct FileObject {
    uri: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

// ---- the inner JSON Gemini is asked to emit -------------------------------

#[derive(Deserialize)]
struct AudioJson {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    segments: Vec<SegJson>,
    summary: SummaryJson,
}

#[derive(Deserialize)]
struct SegJson {
    #[serde(default)]
    start_ms: i64,
    #[serde(default)]
    end_ms: i64,
    #[serde(default)]
    speaker: Option<String>,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct SummaryJson {
    content: String,
    #[serde(default)]
    action_items: Vec<ActionItem>,
    #[serde(default)]
    key_decisions: Vec<KeyDecision>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_segments_and_summary() {
        let json = r##"{
          "language": "zh-TW",
          "segments": [
            {"start_ms": 0, "end_ms": 1500, "speaker": "講者 1", "text": "大家好"},
            {"start_ms": 1500, "end_ms": 3000, "speaker": null, "text": "我們開始"}
          ],
          "summary": {
            "content": "# 摘要\n會議重點。",
            "action_items": [{"task": "寫文件", "owner": "Alice", "deadline": null}],
            "key_decisions": [{"decision": "採用 Tauri", "context": null}]
          }
        }"##;
        let r = parse_audio_response(json, "m1", "2026-06-23T00:00:00Z", "gemini-3.5-flash", Some(42))
            .unwrap();
        assert_eq!(r.language.as_deref(), Some("zh-TW"));
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.segments[0].meeting_id, "m1");
        assert_eq!(r.segments[0].segment_index, 0);
        assert_eq!(r.segments[0].speaker.as_deref(), Some("講者 1"));
        assert_eq!(r.segments[1].speaker, None); // null → None
        assert_eq!(r.segments[1].start_time_ms, 1500);
        assert!(r.summary.content.contains("會議重點"));
        assert_eq!(r.summary.action_items.len(), 1);
        assert_eq!(r.summary.provider, AiProviderKind::Gemini);
        assert_eq!(r.summary.tokens_used, Some(42));
    }

    #[test]
    fn mime_for_maps_common_extensions() {
        assert_eq!(mime_for(Path::new("recording.wav")), "audio/wav");
        assert_eq!(mime_for(Path::new("a.MP3")), "audio/mp3");
        assert_eq!(mime_for(Path::new("voice memo.m4a")), "audio/mp4");
        assert_eq!(mime_for(Path::new("x.flac")), "audio/flac");
        assert_eq!(mime_for(Path::new("noext")), "audio/wav");
    }

    #[test]
    fn tolerates_code_fence() {
        let json = "```json\n{\"language\":\"zh-TW\",\"segments\":[],\"summary\":{\"content\":\"x\"}}\n```";
        let r = parse_audio_response(json, "m", "now", "gemini-3.5-flash", None).unwrap();
        assert!(r.segments.is_empty());
        assert_eq!(r.summary.content, "x");
    }
}
