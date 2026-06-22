//! Gemini (Google) provider (cloud, optional). Uses the Generative Language
//! API `:generateContent` endpoint over raw `reqwest`. The API key comes from
//! the OS keychain ([`crate::ai::keychain`]) and is passed via the `x-goog-api-key`
//! header (not in the URL, so it doesn't leak into logs).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ai::keychain;
use crate::ai::provider::{
    self, AiError, AiProvider, CostEstimate, ModelPricing, Result, SummaryDraft, SummaryTemplate,
    DEFAULT_OUTPUT_TOKEN_BUDGET,
};
use crate::models::AiProviderKind;

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta/models";

/// Default model: the fast, low-cost Flash tier.
pub const DEFAULT_MODEL: &str = "gemini-1.5-flash";

/// Conservative context budget for chunking (Flash/Pro support large windows;
/// stay well under to leave room for the reply).
const ASSUMED_CONTEXT_TOKENS: usize = 120_000;

/// Gemini provider.
pub struct GeminiProvider {
    /// Base URL for the models endpoint (overridable for tests/proxies).
    api_base: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            api_base: API_BASE.to_string(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// USD price per 1M tokens for `model`. Unknown models fall back to the
    /// Flash price so the cost hook always returns an estimate.
    fn pricing(model: &str) -> ModelPricing {
        match model {
            "gemini-1.5-pro" => ModelPricing {
                input_per_mtok: 1.25,
                output_per_mtok: 5.00,
            },
            // gemini-3.5-flash is the multimodal default for transcription; this
            // (Flash-tier) price is a rough estimate for the text cost hook.
            "gemini-3.5-flash" => ModelPricing {
                input_per_mtok: 0.10,
                output_per_mtok: 0.40,
            },
            "gemini-1.5-flash" => ModelPricing {
                input_per_mtok: 0.075,
                output_per_mtok: 0.30,
            },
            _ => ModelPricing {
                input_per_mtok: 0.075,
                output_per_mtok: 0.30,
            },
        }
    }
}

impl Default for GeminiProvider {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL)
    }
}

#[derive(Debug, Serialize)]
struct GenerateContentRequest<'a> {
    contents: Vec<Content<'a>>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
struct Content<'a> {
    role: &'a str,
    parts: Vec<Part<'a>>,
}

#[derive(Debug, Serialize)]
struct Part<'a> {
    text: &'a str,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    /// Ask Gemini for a raw JSON object so parsing is deterministic.
    #[serde(rename = "responseMimeType")]
    response_mime_type: &'static str,
}

#[derive(Debug, Deserialize)]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata", default)]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    #[serde(default)]
    content: Option<ResponseContent>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    #[serde(default)]
    parts: Vec<ResponsePart>,
}

#[derive(Debug, Deserialize)]
struct ResponsePart {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageMetadata {
    #[serde(rename = "totalTokenCount", default)]
    total_token_count: Option<i64>,
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn summarize(
        &self,
        transcript: &str,
        template: SummaryTemplate,
    ) -> Result<SummaryDraft> {
        let api_key = keychain::get_api_key(AiProviderKind::Gemini)
            .map_err(|e| AiError::Parse(format!("keychain error: {e}")))?
            .ok_or(AiError::MissingApiKey("gemini"))?;

        let client = &self.client;
        let model = self.model.as_str();
        let api_key = api_key.as_str();
        // e.g. https://.../models/gemini-1.5-flash:generateContent
        let url = format!("{}/{model}:generateContent", self.api_base.trim_end_matches('/'));

        provider::summarize_with(
            transcript,
            template,
            ASSUMED_CONTEXT_TOKENS,
            AiProviderKind::Gemini,
            model,
            move |prompt: String| {
                let url = url.clone();
                async move {
                    let body = GenerateContentRequest {
                        contents: vec![Content {
                            role: "user",
                            parts: vec![Part { text: &prompt }],
                        }],
                        generation_config: GenerationConfig {
                            response_mime_type: "application/json",
                        },
                    };
                    let resp = client
                        .post(&url)
                        .header("x-goog-api-key", api_key)
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
                    let content = parsed
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
                    if content.is_empty() {
                        return Err(AiError::Parse("empty Gemini candidate text".into()));
                    }
                    let tokens = parsed.usage_metadata.and_then(|u| u.total_token_count);
                    Ok((content, tokens))
                }
            },
        )
        .await
    }

    fn kind(&self) -> AiProviderKind {
        AiProviderKind::Gemini
    }

    fn models(&self) -> Vec<String> {
        vec![
            "gemini-3.5-flash".to_string(),
            "gemini-1.5-flash".to_string(),
            "gemini-1.5-pro".to_string(),
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
        let p = GeminiProvider::default();
        assert_eq!(p.active_model(), DEFAULT_MODEL);
        assert_eq!(p.kind(), AiProviderKind::Gemini);
        assert!(p.kind().is_cloud());
    }

    #[test]
    fn pro_is_pricier_than_flash() {
        let pro = GeminiProvider::pricing("gemini-1.5-pro");
        let flash = GeminiProvider::pricing("gemini-1.5-flash");
        assert!(pro.input_per_mtok > flash.input_per_mtok);
    }

    #[test]
    fn cost_estimate_priced() {
        let p = GeminiProvider::default();
        let est = p.estimate_cost("Alice: hello there\nBob: hi", SummaryTemplate::General);
        assert!(est.usd_cost.unwrap() > 0.0);
    }
}
