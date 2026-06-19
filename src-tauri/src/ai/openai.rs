//! OpenAI provider (cloud, optional). Uses the Chat Completions API
//! (`POST /v1/chat/completions`). API key comes from the OS keychain
//! ([`crate::ai::keychain`]) — never plain text. Cost is estimated before the
//! transcript is sent (PRD §4.6 "成本估算").

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::ai::keychain;
use crate::ai::provider::{
    self, AiError, AiProvider, CostEstimate, ModelPricing, Result, SummaryDraft, SummaryTemplate,
    DEFAULT_OUTPUT_TOKEN_BUDGET,
};
use crate::models::AiProviderKind;

const DEFAULT_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";

/// Default model: a small, cheap, widely-available chat model.
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// Assumed context window for chunking (gpt-4o family is 128k; we stay
/// conservative to leave room for the reply).
const ASSUMED_CONTEXT_TOKENS: usize = 120_000;

/// OpenAI provider.
pub struct OpenAiProvider {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: model.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Override the endpoint (e.g. an Azure/OpenAI-compatible gateway).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// USD price per 1M tokens for `model`. Returns a default for unknown models
    /// so the cost hook always yields *some* estimate.
    fn pricing(model: &str) -> ModelPricing {
        match model {
            "gpt-4o" => ModelPricing {
                input_per_mtok: 2.50,
                output_per_mtok: 10.00,
            },
            "gpt-4o-mini" => ModelPricing {
                input_per_mtok: 0.15,
                output_per_mtok: 0.60,
            },
            _ => ModelPricing {
                input_per_mtok: 0.50,
                output_per_mtok: 1.50,
            },
        }
    }
}

impl Default for OpenAiProvider {
    fn default() -> Self {
        Self::new(DEFAULT_MODEL)
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    /// Ask for a JSON object response so parsing is deterministic.
    response_format: ResponseFormat,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct Usage {
    #[serde(default)]
    total_tokens: Option<i64>,
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn summarize(
        &self,
        transcript: &str,
        template: SummaryTemplate,
    ) -> Result<SummaryDraft> {
        let api_key = keychain::get_api_key(AiProviderKind::OpenAi)
            .map_err(|e| AiError::Parse(format!("keychain error: {e}")))?
            .ok_or(AiError::MissingApiKey("openai"))?;

        let client = &self.client;
        let endpoint = self.endpoint.as_str();
        let model = self.model.as_str();
        let api_key = api_key.as_str();

        provider::summarize_with(
            transcript,
            template,
            ASSUMED_CONTEXT_TOKENS,
            AiProviderKind::OpenAi,
            model,
            move |prompt: String| async move {
                let body = ChatRequest {
                    model,
                    messages: vec![ChatMessage {
                        role: "user",
                        content: &prompt,
                    }],
                    response_format: ResponseFormat { kind: "json_object" },
                };
                let resp = client
                    .post(endpoint)
                    .bearer_auth(api_key)
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
                let parsed: ChatResponse =
                    resp.json().await.map_err(|e| AiError::Parse(e.to_string()))?;
                let content = parsed
                    .choices
                    .into_iter()
                    .next()
                    .map(|c| c.message.content)
                    .ok_or_else(|| AiError::Parse("no choices in OpenAI response".into()))?;
                let tokens = parsed.usage.and_then(|u| u.total_tokens);
                Ok((content, tokens))
            },
        )
        .await
    }

    fn kind(&self) -> AiProviderKind {
        AiProviderKind::OpenAi
    }

    fn models(&self) -> Vec<String> {
        vec![
            "gpt-4o-mini".to_string(),
            "gpt-4o".to_string(),
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
        let p = OpenAiProvider::default();
        assert_eq!(p.active_model(), DEFAULT_MODEL);
        assert_eq!(p.kind(), AiProviderKind::OpenAi);
        assert!(p.kind().is_cloud());
    }

    #[test]
    fn cost_estimate_is_priced_for_cloud() {
        let p = OpenAiProvider::default();
        let est = p.estimate_cost("Alice: hello\nBob: hi there", SummaryTemplate::General);
        assert!(est.input_tokens > 0);
        let cost = est.usd_cost.expect("cloud provider must report a cost");
        assert!(cost > 0.0);
    }

    #[test]
    fn pricing_known_vs_unknown_model() {
        let known = OpenAiProvider::pricing("gpt-4o-mini");
        assert!((known.input_per_mtok - 0.15).abs() < 1e-9);
        let unknown = OpenAiProvider::pricing("some-future-model");
        assert!(unknown.input_per_mtok > 0.0);
    }
}
