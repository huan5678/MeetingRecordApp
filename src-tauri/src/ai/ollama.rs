//! Ollama provider — the LOCAL, DEFAULT backend (PRD §4.6). Talks to a local
//! Ollama server over HTTP at `/api/generate`. No API key, no per-token cost.
//!
//! The transcript never leaves the machine on this path, which is why it's the
//! default. Long transcripts are chunked + map-reduced by
//! [`provider::summarize_with`].

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ai::provider::{
    self, AiError, AiProvider, CostEstimate, Result, SummaryDraft, SummaryTemplate,
    DEFAULT_OUTPUT_TOKEN_BUDGET,
};
use crate::models::AiProviderKind;

/// Default Ollama HTTP endpoint (the server's local bind address).
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Default model. `llama3.1` is a small, broadly-available general model; the
/// user can pick any model they have pulled locally.
pub const DEFAULT_MODEL: &str = "llama3.1";

/// Context window we assume for chunking. Ollama models vary, but 8k is a safe
/// floor for the common small/medium models; oversizing risks truncation.
const ASSUMED_CONTEXT_TOKENS: usize = 8192;

/// Local Ollama provider.
pub struct OllamaProvider {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    /// Construct with an explicit endpoint + model.
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Construct with the default local endpoint and the given model.
    pub fn with_model(model: impl Into<String>) -> Self {
        Self::new(DEFAULT_ENDPOINT, model)
    }
}

impl Default for OllamaProvider {
    fn default() -> Self {
        Self::new(DEFAULT_ENDPOINT, DEFAULT_MODEL)
    }
}

/// Request body for `POST /api/generate`. `stream: false` makes Ollama return a
/// single JSON object instead of a stream of NDJSON chunks.
#[derive(Debug, Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

/// Response body from `POST /api/generate` (the fields we use).
#[derive(Debug, Deserialize)]
struct GenerateResponse {
    response: String,
    /// Tokens in the prompt, when reported.
    #[serde(default)]
    prompt_eval_count: Option<i64>,
    /// Tokens generated, when reported.
    #[serde(default)]
    eval_count: Option<i64>,
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn summarize(
        &self,
        transcript: &str,
        template: SummaryTemplate,
    ) -> Result<SummaryDraft> {
        let endpoint = self.endpoint.trim_end_matches('/');
        let url = format!("{endpoint}/api/generate");
        let client = &self.client;
        let model = self.model.as_str();

        provider::summarize_with(
            transcript,
            template,
            ASSUMED_CONTEXT_TOKENS,
            AiProviderKind::Ollama,
            model,
            move |prompt: String| {
                let url = url.clone();
                async move {
                    let body = GenerateRequest {
                        model,
                        prompt: &prompt,
                        stream: false,
                    };
                    let resp = client.post(&url).json(&body).send().await?;
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        return Err(AiError::Status {
                            status: status.as_u16(),
                            body: truncate(&text, 512),
                        });
                    }
                    let parsed: GenerateResponse = resp
                        .json()
                        .await
                        .map_err(|e| AiError::Parse(e.to_string()))?;
                    let tokens = match (parsed.prompt_eval_count, parsed.eval_count) {
                        (None, None) => None,
                        (p, e) => Some(p.unwrap_or(0) + e.unwrap_or(0)),
                    };
                    Ok((parsed.response, tokens))
                }
            },
        )
        .await
    }

    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Ollama
    }

    fn models(&self) -> Vec<String> {
        // We can't enumerate the user's pulled models without a network call;
        // surface common defaults plus whatever is configured.
        let mut models = vec![
            DEFAULT_MODEL.to_string(),
            "llama3.2".to_string(),
            "qwen2.5".to_string(),
            "mistral".to_string(),
            "gemma2".to_string(),
        ];
        if !models.iter().any(|m| m == &self.model) {
            models.insert(0, self.model.clone());
        }
        models
    }

    fn active_model(&self) -> &str {
        &self.model
    }

    /// Local — no cost. Still reports an input-token estimate for parity with
    /// the cloud providers' UI.
    fn estimate_cost(&self, transcript: &str, template: SummaryTemplate) -> CostEstimate {
        let prompt = provider::build_prompt(transcript, template);
        CostEstimate {
            input_tokens: provider::estimate_tokens(&prompt),
            output_tokens: DEFAULT_OUTPUT_TOKEN_BUDGET,
            usd_cost: None,
        }
    }
}

/// Truncate a diagnostic string to `max` chars (avoids dumping huge error
/// bodies into logs).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::AiProvider;

    #[test]
    fn default_provider_uses_local_endpoint_and_default_model() {
        let p = OllamaProvider::default();
        assert_eq!(p.active_model(), DEFAULT_MODEL);
        assert_eq!(p.kind(), AiProviderKind::Ollama);
        assert_eq!(p.name(), "ollama");
        assert_eq!(p.endpoint, DEFAULT_ENDPOINT);
    }

    #[test]
    fn models_include_configured_model_first_when_custom() {
        let p = OllamaProvider::with_model("my-custom-model");
        let models = p.models();
        assert_eq!(models.first().map(String::as_str), Some("my-custom-model"));
    }

    #[test]
    fn cost_estimate_is_free_for_local() {
        let p = OllamaProvider::default();
        let est = p.estimate_cost("Alice: hello\nBob: hi", SummaryTemplate::General);
        assert_eq!(est.usd_cost, None);
        assert!(est.input_tokens > 0);
    }

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("short", 512), "short");
        let long = "x".repeat(600);
        let t = truncate(&long, 512);
        assert!(t.chars().count() <= 513); // 512 + ellipsis
        assert!(t.ends_with('…'));
    }
}
