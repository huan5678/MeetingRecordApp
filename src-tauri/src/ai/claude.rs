//! Claude (Anthropic) provider (cloud, optional). Uses the Messages API
//! (`POST /v1/messages`) over raw `reqwest` — Rust has no official Anthropic
//! SDK, and the AI module is provider-neutral by design (PRD §4.6). Required
//! headers: `x-api-key` + `anthropic-version: 2023-06-01`. The API key comes
//! from the OS keychain ([`crate::ai::keychain`]).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ai::keychain;
use crate::ai::provider::{
    self, AiError, AiProvider, CostEstimate, ModelPricing, Result, SummaryDraft, SummaryTemplate,
    DEFAULT_OUTPUT_TOKEN_BUDGET,
};
use crate::models::AiProviderKind;

const DEFAULT_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// The Messages API version header value (stable, date-pinned per Anthropic).
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default model. For meeting summarization we default to the fast, low-cost
/// Haiku tier; the user can switch to a Sonnet/Opus model in settings for
/// higher quality. (Model IDs are exact — no date suffixes.)
pub const DEFAULT_MODEL: &str = "claude-haiku-4-5";

/// Conservative context budget for chunking. The current Claude models have
/// large windows; 180k leaves ample headroom for the reply.
const ASSUMED_CONTEXT_TOKENS: usize = 180_000;

/// Per-request output cap. The Messages API requires `max_tokens`.
const MAX_OUTPUT_TOKENS: u32 = 1500;

/// Claude provider.
pub struct ClaudeProvider {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeProvider {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// USD price per 1M tokens for `model`. Unknown models get a mid-tier
    /// default so the cost hook always returns an estimate.
    fn pricing(model: &str) -> ModelPricing {
        match model {
            "claude-opus-4-8" => ModelPricing {
                input_per_mtok: 5.00,
                output_per_mtok: 25.00,
            },
            "claude-sonnet-4-6" => ModelPricing {
                input_per_mtok: 3.00,
                output_per_mtok: 15.00,
            },
            "claude-haiku-4-5" => ModelPricing {
                input_per_mtok: 1.00,
                output_per_mtok: 5.00,
            },
            _ => ModelPricing {
                input_per_mtok: 3.00,
                output_per_mtok: 15.00,
            },
        }
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL)
    }
}

#[derive(Debug, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<Message<'a>>,
}

#[derive(Debug, Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn summarize(
        &self,
        transcript: &str,
        template: SummaryTemplate,
    ) -> Result<SummaryDraft> {
        let api_key = keychain::get_api_key(AiProviderKind::Claude)
            .map_err(|e| AiError::Parse(format!("keychain error: {e}")))?
            .ok_or(AiError::MissingApiKey("claude"))?;

        let client = &self.client;
        let endpoint = self.endpoint.as_str();
        let model = self.model.as_str();
        let api_key = api_key.as_str();

        provider::summarize_with(
            transcript,
            template,
            ASSUMED_CONTEXT_TOKENS,
            AiProviderKind::Claude,
            model,
            move |prompt: String| async move {
                let body = MessagesRequest {
                    model,
                    max_tokens: MAX_OUTPUT_TOKENS,
                    messages: vec![Message {
                        role: "user",
                        content: &prompt,
                    }],
                };
                let resp = client
                    .post(endpoint)
                    .header("x-api-key", api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
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
                let parsed: MessagesResponse =
                    resp.json().await.map_err(|e| AiError::Parse(e.to_string()))?;
                // Concatenate all text blocks (the API may emit several).
                let content = parsed
                    .content
                    .into_iter()
                    .filter(|b| b.kind == "text")
                    .filter_map(|b| b.text)
                    .collect::<Vec<_>>()
                    .join("");
                if content.is_empty() {
                    return Err(AiError::Parse("no text content in Claude response".into()));
                }
                let tokens = parsed.usage.map(|u| {
                    u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0)
                });
                Ok((content, tokens))
            },
        )
        .await
    }

    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Claude
    }

    fn models(&self) -> Vec<String> {
        vec![
            "claude-haiku-4-5".to_string(),
            "claude-sonnet-4-6".to_string(),
            "claude-opus-4-8".to_string(),
        ]
    }

    fn active_model(&self) -> &str {
        &self.model
    }

    fn estimate_cost(&self, transcript: &str, template: SummaryTemplate) -> CostEstimate {
        let prompt = provider::build_prompt(transcript, template);
        let input_tokens = provider::estimate_tokens(&prompt);
        let output_tokens = DEFAULT_OUTPUT_TOKEN_BUDGET;
        let pricing = Self::pricing(&self.model);
        CostEstimate {
            input_tokens,
            output_tokens,
            usd_cost: Some(pricing.cost(input_tokens, output_tokens)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::AiProvider;

    #[test]
    fn defaults() {
        let p = ClaudeProvider::default();
        assert_eq!(p.active_model(), DEFAULT_MODEL);
        assert_eq!(p.kind(), AiProviderKind::Claude);
        assert!(p.kind().is_cloud());
    }

    #[test]
    fn model_ids_have_no_date_suffix() {
        // Guard against accidentally introducing dated IDs (which 404).
        let p = ClaudeProvider::default();
        for m in p.models() {
            assert!(
                !m.chars().rev().take(8).all(|c| c.is_ascii_digit() || c == '-'),
                "model id looks date-suffixed: {m}"
            );
        }
    }

    #[test]
    fn opus_is_pricier_than_haiku() {
        let opus = ClaudeProvider::pricing("claude-opus-4-8");
        let haiku = ClaudeProvider::pricing("claude-haiku-4-5");
        assert!(opus.input_per_mtok > haiku.input_per_mtok);
        assert!(opus.output_per_mtok > haiku.output_per_mtok);
    }

    #[test]
    fn cost_estimate_priced() {
        let p = ClaudeProvider::default();
        let est = p.estimate_cost("a long enough transcript line here", SummaryTemplate::General);
        assert!(est.usd_cost.unwrap() > 0.0);
    }
}
