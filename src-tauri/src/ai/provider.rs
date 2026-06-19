//! `AiProvider` trait + shared summary/cost types and the prompt/chunking
//! machinery every provider reuses (PRD §4.6).
//!
//! Design notes:
//! - [`AiProvider`] is the abstraction over Ollama (local, default) and the
//!   cloud providers (OpenAI / Claude / Gemini). It exposes `summarize`,
//!   `name`, `models`, and a cost-estimate hook.
//! - [`SummaryTemplate`] enumerates the five meeting templates from the PRD and
//!   maps a [`crate::models::MeetingType`] onto one.
//! - [`build_prompt`] assembles the instruction + transcript into the text sent
//!   to a model. [`chunk_transcript`] + the map-reduce helpers handle
//!   transcripts that exceed a model's context window.
//! - Providers return a [`SummaryDraft`] — the *content* of a summary without
//!   the storage-owned fields (`id`, `meeting_id`, `created_at`). The Integrate
//!   / storage phase maps a draft into [`crate::models::Summary`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::models::{ActionItem, AiProviderKind, KeyDecision, MeetingType};

/// Errors surfaced by the AI summarization layer.
#[derive(Debug, thiserror::Error)]
pub enum AiError {
    /// The HTTP request to a provider failed (network, TLS, timeout, …).
    #[error("AI provider request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// The provider returned a non-success status with a (possibly truncated)
    /// body for diagnostics.
    #[error("AI provider returned status {status}: {body}")]
    Status { status: u16, body: String },

    /// The provider response could not be parsed into the expected shape.
    #[error("failed to parse AI provider response: {0}")]
    Parse(String),

    /// A cloud provider needs an API key but none was found in the keychain.
    #[error("missing API key for {0} (store it in the OS keychain first)")]
    MissingApiKey(&'static str),

    /// The transcript was empty — nothing to summarize.
    #[error("cannot summarize an empty transcript")]
    EmptyTranscript,
}

/// Result alias for the AI layer.
pub type Result<T> = std::result::Result<T, AiError>;

/// The five summary templates from PRD §4.6. Selecting a template swaps the
/// instruction block in the prompt so the structure matches the meeting type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryTemplate {
    /// 1:1 Meeting: discussion topics, action items, follow-ups.
    OneOnOne,
    /// Team Sync: updates from each member, blockers, action items.
    TeamSync,
    /// Client Call: client needs, proposed solutions, next steps.
    ClientCall,
    /// Interview: candidate assessment, strengths/weaknesses, recommendation.
    Interview,
    /// General: key points, decisions, action items, open questions.
    General,
}

impl Default for SummaryTemplate {
    fn default() -> Self {
        SummaryTemplate::General
    }
}

impl SummaryTemplate {
    /// Map a meeting type onto its default template. `None` (untyped meeting)
    /// falls back to [`SummaryTemplate::General`].
    pub fn for_meeting_type(meeting_type: Option<MeetingType>) -> Self {
        match meeting_type {
            Some(MeetingType::OneOnOne) => SummaryTemplate::OneOnOne,
            Some(MeetingType::TeamSync) => SummaryTemplate::TeamSync,
            Some(MeetingType::ClientCall) => SummaryTemplate::ClientCall,
            Some(MeetingType::Interview) => SummaryTemplate::Interview,
            Some(MeetingType::Other) | None => SummaryTemplate::General,
        }
    }

    /// A short human label (used in UI / logs).
    pub fn label(self) -> &'static str {
        match self {
            SummaryTemplate::OneOnOne => "1:1 Meeting",
            SummaryTemplate::TeamSync => "Team Sync",
            SummaryTemplate::ClientCall => "Client Call",
            SummaryTemplate::Interview => "Interview",
            SummaryTemplate::General => "General",
        }
    }

    /// The template-specific instruction block injected into the prompt. It
    /// tells the model what sections to focus on for this meeting type. The
    /// JSON-output contract (in [`OUTPUT_CONTRACT`]) is appended separately so
    /// every template parses the same way.
    pub fn instructions(self) -> &'static str {
        match self {
            SummaryTemplate::OneOnOne => {
                "This is a 1:1 meeting. Focus on the discussion topics raised, \
                 concrete action items with their owners, and follow-ups agreed \
                 for next time."
            }
            SummaryTemplate::TeamSync => {
                "This is a team sync. Capture the status update from each member, \
                 call out any blockers or risks, and list the action items with \
                 owners."
            }
            SummaryTemplate::ClientCall => {
                "This is a client call. Capture the client's needs and concerns, \
                 the solutions proposed, and the agreed next steps."
            }
            SummaryTemplate::Interview => {
                "This is an interview. Summarize the candidate assessment, their \
                 strengths and weaknesses, and a clear hiring recommendation."
            }
            SummaryTemplate::General => {
                "Summarize this meeting. Capture the key points discussed, the \
                 decisions made, the action items with owners, and any open \
                 questions."
            }
        }
    }
}

/// The structured-output contract appended to every prompt. Asking for a single
/// fenced JSON object keeps parsing deterministic across providers (local
/// Ollama models included), while still letting `content` hold rich Markdown.
pub const OUTPUT_CONTRACT: &str = "\
Respond with a SINGLE JSON object and nothing else (no prose before or after, \
no markdown code fence). The object must have exactly these keys:
  \"content\": a Markdown string summarizing the meeting,
  \"action_items\": an array of objects, each { \"task\": string, \"owner\": string|null, \"deadline\": string|null },
  \"key_decisions\": an array of objects, each { \"decision\": string, \"context\": string|null }.
Use null (not the empty string) when an owner, deadline, or context is unknown. \
If there are no action items or decisions, use an empty array.";

/// The content of an AI-generated summary, without the storage-owned fields
/// (`id`, `meeting_id`, `created_at`). The storage layer maps this into a
/// [`crate::models::Summary`] row when persisting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryDraft {
    /// Markdown body.
    pub content: String,
    /// Extracted action items.
    #[serde(default)]
    pub action_items: Vec<ActionItem>,
    /// Extracted key decisions.
    #[serde(default)]
    pub key_decisions: Vec<KeyDecision>,
    /// Which provider produced this (for the `summaries.ai_provider` column).
    pub provider: AiProviderKind,
    /// The specific model used (for `summaries.ai_model`).
    pub model: String,
    /// Total tokens used, when the provider reports it. Ollama and the
    /// map-reduce path may leave this `None`.
    #[serde(default)]
    pub tokens_used: Option<i64>,
}

/// The raw JSON object a model is asked to emit (see [`OUTPUT_CONTRACT`]). This
/// is the parse target before it's lifted into a [`SummaryDraft`].
#[derive(Debug, Clone, Deserialize)]
struct RawSummary {
    content: String,
    #[serde(default)]
    action_items: Vec<ActionItem>,
    #[serde(default)]
    key_decisions: Vec<KeyDecision>,
}

/// A rough token estimate for `text`. We use a ~4-chars-per-token heuristic,
/// which is good enough for context-window budgeting and the cloud cost hook
/// (we are not billing against it). Always returns at least 1 for non-empty
/// input so a tiny transcript never estimates as zero tokens.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.chars().count().div_ceil(4).max(1)
}

/// A token + cost estimate for a planned summarization request. Cloud providers
/// fill `usd_cost`; local Ollama returns `None` cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated prompt (input) tokens.
    pub input_tokens: usize,
    /// Estimated completion (output) tokens — a fixed budget for the summary.
    pub output_tokens: usize,
    /// Estimated cost in USD, or `None` for local providers (no cost).
    pub usd_cost: Option<f64>,
}

/// USD price per 1M input / output tokens for a cloud model. Used by the
/// cost-estimate hook. Local providers don't define one.
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

impl ModelPricing {
    /// Cost in USD for the given token counts.
    pub fn cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        (input_tokens as f64 / 1_000_000.0) * self.input_per_mtok
            + (output_tokens as f64 / 1_000_000.0) * self.output_per_mtok
    }
}

/// Default completion-token budget we assume a summary will use, for the cost
/// estimate. The actual cap is set per-provider on the request.
pub const DEFAULT_OUTPUT_TOKEN_BUDGET: usize = 1024;

/// Abstraction over a summarization backend (PRD §4.6).
///
/// Implementors: [`crate::ai::ollama::OllamaProvider`] (local, default),
/// [`crate::ai::openai::OpenAiProvider`], [`crate::ai::claude::ClaudeProvider`],
/// [`crate::ai::gemini::GeminiProvider`] (cloud, optional).
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Summarize `transcript` using `template`. Implementations should call
    /// [`summarize_with`] (below) so long transcripts get chunked
    /// + map-reduced uniformly; they only supply the per-call completion.
    async fn summarize(
        &self,
        transcript: &str,
        template: SummaryTemplate,
    ) -> Result<SummaryDraft>;

    /// Provider kind (drives the `summaries.ai_provider` column).
    fn kind(&self) -> AiProviderKind;

    /// Stable provider name for display/logging (e.g. "ollama", "openai").
    fn name(&self) -> &str {
        self.kind().as_db_str()
    }

    /// Model identifiers this provider can use, for the settings UI.
    fn models(&self) -> Vec<String>;

    /// The model that will actually be used for the next request.
    fn active_model(&self) -> &str;

    /// Token + cost estimate for summarizing `transcript`. Cloud providers
    /// override the default to fill `usd_cost`; the local default returns
    /// `None` cost. Shown to the user before sending a transcript to the cloud
    /// (PRD §4.6 "成本估算").
    fn estimate_cost(&self, transcript: &str, template: SummaryTemplate) -> CostEstimate {
        let prompt = build_prompt(transcript, template);
        CostEstimate {
            input_tokens: estimate_tokens(&prompt),
            output_tokens: DEFAULT_OUTPUT_TOKEN_BUDGET,
            usd_cost: None,
        }
    }
}

/// Build the full prompt sent to a model for a single (already context-sized)
/// transcript chunk. Layout: template instructions → output contract →
/// transcript, clearly delimited so the model doesn't confuse instructions with
/// transcript content.
pub fn build_prompt(transcript: &str, template: SummaryTemplate) -> String {
    format!(
        "{instructions}\n\n{contract}\n\n--- TRANSCRIPT START ---\n{transcript}\n--- TRANSCRIPT END ---",
        instructions = template.instructions(),
        contract = OUTPUT_CONTRACT,
        transcript = transcript.trim(),
    )
}

/// Build the map-reduce *reduce* prompt: given several per-chunk summaries
/// (already JSON or Markdown), ask the model to merge them into one coherent
/// summary under the same template + output contract.
pub fn build_reduce_prompt(partial_summaries: &[String], template: SummaryTemplate) -> String {
    let joined = partial_summaries
        .iter()
        .enumerate()
        .map(|(i, s)| format!("## Partial summary {}\n{}", i + 1, s.trim()))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{instructions}\n\nYou are given several partial summaries of \
         consecutive segments of the SAME meeting. Merge them into one \
         de-duplicated summary. Combine overlapping action items and decisions \
         rather than repeating them.\n\n{contract}\n\n--- PARTIAL SUMMARIES START ---\n{joined}\n--- PARTIAL SUMMARIES END ---",
        instructions = template.instructions(),
        contract = OUTPUT_CONTRACT,
        joined = joined,
    )
}

/// Split a transcript into chunks that each fit within `max_input_tokens` once
/// the prompt scaffolding is accounted for. Splits on line boundaries (a
/// transcript is one segment per line) so we never cut a sentence mid-word; a
/// single over-long line is hard-split by characters as a last resort.
///
/// Returns at least one chunk for any non-empty transcript.
pub fn chunk_transcript(transcript: &str, max_input_tokens: usize) -> Vec<String> {
    // Reserve headroom for the instructions + output contract that wrap each
    // chunk so the *whole prompt* stays under the model's window.
    let scaffold_tokens = estimate_tokens(OUTPUT_CONTRACT) + 256;
    let budget = max_input_tokens.saturating_sub(scaffold_tokens).max(128);

    let trimmed = transcript.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if estimate_tokens(trimmed) <= budget {
        return vec![trimmed.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0usize;

    for line in trimmed.lines() {
        let line_tokens = estimate_tokens(line) + 1; // +1 for the newline

        // A single line larger than the budget: flush what we have, then
        // hard-split the line by characters.
        if line_tokens > budget {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
                current_tokens = 0;
            }
            for piece in hard_split(line, budget) {
                chunks.push(piece);
            }
            continue;
        }

        if current_tokens + line_tokens > budget && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_tokens = 0;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
        current_tokens += line_tokens;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Hard-split a single over-long line into chunks of at most `budget` tokens,
/// on character boundaries.
fn hard_split(line: &str, budget: usize) -> Vec<String> {
    let max_chars = budget.saturating_mul(4).max(4); // inverse of estimate_tokens
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in line.chars() {
        buf.push(ch);
        if buf.chars().count() >= max_chars {
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// Parse a model's text response into a [`SummaryDraft`]. Tolerates models that
/// wrap the JSON in a ```json fence or add stray prose, by extracting the first
/// balanced `{ … }` object. Falls back to treating the whole response as the
/// Markdown `content` when no JSON object is present, so a summary is never lost.
pub fn parse_summary(
    raw_response: &str,
    provider: AiProviderKind,
    model: &str,
    tokens_used: Option<i64>,
) -> Result<SummaryDraft> {
    let response = raw_response.trim();
    if response.is_empty() {
        return Err(AiError::Parse("provider returned an empty response".into()));
    }

    if let Some(json) = extract_json_object(response) {
        if let Ok(raw) = serde_json::from_str::<RawSummary>(json) {
            return Ok(SummaryDraft {
                content: raw.content.trim().to_string(),
                action_items: raw.action_items,
                key_decisions: raw.key_decisions,
                provider,
                model: model.to_string(),
                tokens_used,
            });
        }
    }

    // No parseable JSON object: keep the prose as the Markdown body rather than
    // failing the whole summarization.
    Ok(SummaryDraft {
        content: response.to_string(),
        action_items: Vec::new(),
        key_decisions: Vec::new(),
        provider,
        model: model.to_string(),
        tokens_used,
    })
}

/// Extract the first balanced top-level JSON object from `text`, ignoring
/// braces inside string literals. Returns the `{ … }` slice or `None`.
fn extract_json_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The driver every provider reuses: chunk the transcript, summarize each chunk
/// via the supplied async `complete` closure, then (for >1 chunk) reduce the
/// partial summaries into one. `complete(prompt)` returns the model's raw text.
///
/// This keeps chunking / map-reduce logic in one place; providers only supply
/// the HTTP call.
pub async fn summarize_with<F, Fut>(
    transcript: &str,
    template: SummaryTemplate,
    max_input_tokens: usize,
    provider: AiProviderKind,
    model: &str,
    complete: F,
) -> Result<SummaryDraft>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<(String, Option<i64>)>>,
{
    let chunks = chunk_transcript(transcript, max_input_tokens);
    if chunks.is_empty() {
        return Err(AiError::EmptyTranscript);
    }

    // Single chunk: one call, done.
    if chunks.len() == 1 {
        let prompt = build_prompt(&chunks[0], template);
        let (text, tokens) = complete(prompt).await?;
        return parse_summary(&text, provider, model, tokens);
    }

    // Map: summarize each chunk.
    let mut partials = Vec::with_capacity(chunks.len());
    let mut total_tokens: Option<i64> = None;
    for chunk in &chunks {
        let prompt = build_prompt(chunk, template);
        let (text, tokens) = complete(prompt).await?;
        if let Some(t) = tokens {
            total_tokens = Some(total_tokens.unwrap_or(0) + t);
        }
        partials.push(text);
    }

    // Reduce: merge the partials into one summary.
    let reduce_prompt = build_reduce_prompt(&partials, template);
    let (reduced_text, reduce_tokens) = complete(reduce_prompt).await?;
    if let Some(t) = reduce_tokens {
        total_tokens = Some(total_tokens.unwrap_or(0) + t);
    }
    parse_summary(&reduced_text, provider, model, total_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_selection_from_meeting_type() {
        assert_eq!(
            SummaryTemplate::for_meeting_type(Some(MeetingType::OneOnOne)),
            SummaryTemplate::OneOnOne
        );
        assert_eq!(
            SummaryTemplate::for_meeting_type(Some(MeetingType::TeamSync)),
            SummaryTemplate::TeamSync
        );
        assert_eq!(
            SummaryTemplate::for_meeting_type(Some(MeetingType::ClientCall)),
            SummaryTemplate::ClientCall
        );
        assert_eq!(
            SummaryTemplate::for_meeting_type(Some(MeetingType::Interview)),
            SummaryTemplate::Interview
        );
        // Other and untyped both fall back to General.
        assert_eq!(
            SummaryTemplate::for_meeting_type(Some(MeetingType::Other)),
            SummaryTemplate::General
        );
        assert_eq!(
            SummaryTemplate::for_meeting_type(None),
            SummaryTemplate::General
        );
    }

    #[test]
    fn template_default_is_general() {
        assert_eq!(SummaryTemplate::default(), SummaryTemplate::General);
    }

    #[test]
    fn build_prompt_includes_instructions_contract_and_transcript() {
        let prompt = build_prompt("Alice: hello\nBob: hi", SummaryTemplate::OneOnOne);
        // Template-specific instruction text.
        assert!(prompt.contains("1:1 meeting"));
        // Output contract.
        assert!(prompt.contains("\"action_items\""));
        assert!(prompt.contains("\"key_decisions\""));
        // Transcript delimited.
        assert!(prompt.contains("--- TRANSCRIPT START ---"));
        assert!(prompt.contains("Alice: hello"));
        assert!(prompt.contains("--- TRANSCRIPT END ---"));
        // Instructions come before the transcript.
        assert!(prompt.find("1:1 meeting").unwrap() < prompt.find("Alice: hello").unwrap());
    }

    #[test]
    fn build_prompt_differs_per_template() {
        let t = "x";
        assert_ne!(
            build_prompt(t, SummaryTemplate::Interview),
            build_prompt(t, SummaryTemplate::TeamSync)
        );
        assert!(build_prompt(t, SummaryTemplate::Interview).contains("candidate"));
        assert!(build_prompt(t, SummaryTemplate::TeamSync).contains("blockers"));
    }

    #[test]
    fn estimate_tokens_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        // 4 chars -> 1 token.
        assert_eq!(estimate_tokens("abcd"), 1);
        // 5 chars -> ceil(5/4) = 2 tokens.
        assert_eq!(estimate_tokens("abcde"), 2);
        // non-empty is always >= 1.
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn chunk_empty_transcript_yields_no_chunks() {
        assert!(chunk_transcript("", 4096).is_empty());
        assert!(chunk_transcript("   \n  \n", 4096).is_empty());
    }

    #[test]
    fn chunk_short_transcript_is_single_chunk() {
        let chunks = chunk_transcript("Alice: hello\nBob: hi there", 4096);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Alice: hello"));
        assert!(chunks[0].contains("Bob: hi there"));
    }

    #[test]
    fn chunk_long_transcript_splits_on_line_boundaries() {
        // Build a transcript that clearly exceeds a small budget. Each line is
        // ~50 chars (~13 tokens); with a tiny window we force several chunks.
        let line = "Speaker A: this is a reasonably long line of dialogue.";
        let transcript = std::iter::repeat(line)
            .take(200)
            .collect::<Vec<_>>()
            .join("\n");

        let chunks = chunk_transcript(&transcript, 512);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());

        // No content is lost: every original line appears across the chunks,
        // and no line is split (each chunk line equals the original line).
        let total_lines: usize = chunks.iter().map(|c| c.lines().count()).sum();
        assert_eq!(total_lines, 200);
        for chunk in &chunks {
            for l in chunk.lines() {
                assert_eq!(l, line, "a line was split across chunks");
            }
        }

        // Each chunk respects the budget (with scaffold headroom subtracted).
        let scaffold = estimate_tokens(OUTPUT_CONTRACT) + 256;
        let budget = (512usize).saturating_sub(scaffold).max(128);
        for chunk in &chunks {
            assert!(
                estimate_tokens(chunk) <= budget,
                "chunk exceeded budget: {} > {}",
                estimate_tokens(chunk),
                budget
            );
        }
    }

    #[test]
    fn chunk_hard_splits_a_single_overlong_line() {
        // One line with no newlines, far larger than the budget.
        let huge = "z".repeat(20_000);
        let chunks = chunk_transcript(&huge, 256);
        assert!(chunks.len() > 1);
        // Reassembled, no characters are lost.
        let rejoined: String = chunks.concat();
        assert_eq!(rejoined.len(), huge.len());
    }

    #[test]
    fn parse_summary_from_clean_json() {
        let json = r##"{
            "content": "# Summary\nWe shipped v1.0.",
            "action_items": [
                {"task": "write docs", "owner": "Alice", "deadline": "2026-07-01"},
                {"task": "fix bug"}
            ],
            "key_decisions": [
                {"decision": "Use Tauri", "context": "smaller binary"}
            ]
        }"##;
        let draft = parse_summary(json, AiProviderKind::Ollama, "llama3", Some(42)).unwrap();
        assert!(draft.content.contains("shipped v1.0"));
        assert_eq!(draft.action_items.len(), 2);
        assert_eq!(draft.action_items[0].owner.as_deref(), Some("Alice"));
        assert_eq!(draft.action_items[0].deadline.as_deref(), Some("2026-07-01"));
        // Defaulted optional fields on the minimal item.
        assert_eq!(draft.action_items[1].task, "fix bug");
        assert_eq!(draft.action_items[1].owner, None);
        assert!(!draft.action_items[1].done);
        assert_eq!(draft.key_decisions.len(), 1);
        assert_eq!(draft.key_decisions[0].decision, "Use Tauri");
        assert_eq!(draft.provider, AiProviderKind::Ollama);
        assert_eq!(draft.model, "llama3");
        assert_eq!(draft.tokens_used, Some(42));
    }

    #[test]
    fn parse_summary_tolerates_code_fence_and_prose() {
        let raw = "Sure! Here is the summary:\n```json\n{\"content\":\"hi\",\"action_items\":[],\"key_decisions\":[]}\n```\nLet me know if you need more.";
        let draft = parse_summary(raw, AiProviderKind::OpenAi, "gpt", None).unwrap();
        assert_eq!(draft.content, "hi");
        assert!(draft.action_items.is_empty());
        assert!(draft.key_decisions.is_empty());
    }

    #[test]
    fn parse_summary_handles_braces_inside_strings() {
        let raw = r#"{"content":"use a struct { field: T } here","action_items":[],"key_decisions":[]}"#;
        let draft = parse_summary(raw, AiProviderKind::Claude, "claude", None).unwrap();
        assert_eq!(draft.content, "use a struct { field: T } here");
    }

    #[test]
    fn parse_summary_falls_back_to_markdown_when_no_json() {
        let raw = "# Meeting Notes\n\nWe discussed the roadmap.";
        let draft = parse_summary(raw, AiProviderKind::Gemini, "gemini", None).unwrap();
        assert_eq!(draft.content, raw);
        assert!(draft.action_items.is_empty());
    }

    #[test]
    fn parse_summary_rejects_empty_response() {
        let err = parse_summary("   ", AiProviderKind::Ollama, "m", None).unwrap_err();
        assert!(matches!(err, AiError::Parse(_)));
    }

    #[test]
    fn build_reduce_prompt_lists_all_partials() {
        let partials = vec!["first part".to_string(), "second part".to_string()];
        let prompt = build_reduce_prompt(&partials, SummaryTemplate::General);
        assert!(prompt.contains("Partial summary 1"));
        assert!(prompt.contains("Partial summary 2"));
        assert!(prompt.contains("first part"));
        assert!(prompt.contains("second part"));
        assert!(prompt.contains("Merge them into one"));
    }

    #[test]
    fn model_pricing_cost_math() {
        let pricing = ModelPricing {
            input_per_mtok: 5.0,
            output_per_mtok: 25.0,
        };
        // 1M input + 1M output = 5 + 25 = 30.
        assert!((pricing.cost(1_000_000, 1_000_000) - 30.0).abs() < 1e-9);
        // 200k input, 0 output = 1.0.
        assert!((pricing.cost(200_000, 0) - 1.0).abs() < 1e-9);
    }

    // --- map-reduce driver (no network — mock `complete`) -------------------

    use std::cell::RefCell;

    /// Records every prompt the driver sends, and returns a canned JSON summary
    /// for each call. Single-threaded test, so a thread-local counter is fine.
    fn canned_complete(
        calls: &RefCell<Vec<String>>,
    ) -> impl Fn(String) -> std::future::Ready<Result<(String, Option<i64>)>> + '_ {
        move |prompt: String| {
            calls.borrow_mut().push(prompt);
            let n = calls.borrow().len();
            let json = format!(
                r#"{{"content":"summary chunk {n}","action_items":[{{"task":"task {n}"}}],"key_decisions":[]}}"#
            );
            std::future::ready(Ok((json, Some(10))))
        }
    }

    #[tokio::test]
    async fn summarize_with_single_chunk_makes_one_call() {
        let calls = RefCell::new(Vec::new());
        let draft = summarize_with(
            "Alice: hi\nBob: hello",
            SummaryTemplate::General,
            4096,
            AiProviderKind::Ollama,
            "test-model",
            canned_complete(&calls),
        )
        .await
        .unwrap();

        // Exactly one model call for a short transcript (no reduce step).
        assert_eq!(calls.borrow().len(), 1);
        // The single call is a per-chunk summary prompt, not a reduce prompt.
        assert!(calls.borrow()[0].contains("--- TRANSCRIPT START ---"));
        assert_eq!(draft.content, "summary chunk 1");
        assert_eq!(draft.action_items.len(), 1);
        assert_eq!(draft.provider, AiProviderKind::Ollama);
        assert_eq!(draft.model, "test-model");
        assert_eq!(draft.tokens_used, Some(10));
    }

    #[tokio::test]
    async fn summarize_with_long_transcript_maps_then_reduces() {
        // Force several chunks with a tiny window.
        let line = "Speaker A: this is a reasonably long line of dialogue here.";
        let transcript = std::iter::repeat(line)
            .take(300)
            .collect::<Vec<_>>()
            .join("\n");

        let n_chunks = chunk_transcript(&transcript, 512).len();
        assert!(n_chunks > 1, "test needs multiple chunks");

        let calls = RefCell::new(Vec::new());
        let draft = summarize_with(
            &transcript,
            SummaryTemplate::TeamSync,
            512,
            AiProviderKind::Claude,
            "test-model",
            canned_complete(&calls),
        )
        .await
        .unwrap();

        // One call per chunk (map) + one reduce call.
        assert_eq!(calls.borrow().len(), n_chunks + 1);

        // The final call is the reduce prompt (merges partials).
        let last = calls.borrow().last().unwrap().clone();
        assert!(last.contains("--- PARTIAL SUMMARIES START ---"));
        assert!(last.contains("Merge them into one"));

        // The driver returns the reduced summary (last canned call).
        assert_eq!(draft.content, format!("summary chunk {}", n_chunks + 1));
        // Tokens are summed across all map calls + the reduce call.
        assert_eq!(draft.tokens_used, Some(10 * (n_chunks as i64 + 1)));
    }

    #[tokio::test]
    async fn summarize_with_empty_transcript_errors() {
        let calls = RefCell::new(Vec::new());
        let err = summarize_with(
            "   \n  ",
            SummaryTemplate::General,
            4096,
            AiProviderKind::Ollama,
            "m",
            canned_complete(&calls),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AiError::EmptyTranscript));
        assert_eq!(calls.borrow().len(), 0);
    }

    #[tokio::test]
    async fn summarize_with_propagates_provider_errors() {
        let complete = |_prompt: String| {
            std::future::ready(Err(AiError::Status {
                status: 503,
                body: "service unavailable".into(),
            }))
        };
        let err = summarize_with(
            "Alice: hi",
            SummaryTemplate::General,
            4096,
            AiProviderKind::OpenAi,
            "m",
            complete,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AiError::Status { status: 503, .. }));
    }
}
