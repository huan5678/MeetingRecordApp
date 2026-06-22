//! AI summarization module.
//!
//! Provider trait ([`provider::AiProvider`]) with Ollama (local, DEFAULT, no
//! API key) + OpenAI / Claude / Gemini (cloud, optional) implementations. Long
//! transcripts are handled by chunking + map-reduce ([`provider::summarize_with`],
//! [`provider::chunk_transcript`]); cloud providers expose a token/cost estimate
//! hook ([`provider::AiProvider::estimate_cost`]). Summary templates per meeting
//! type live in [`templates`]. Cloud API keys come from the OS keychain
//! ([`keychain`]), never plain text. See docs/PRD.md §4.6.

pub mod claude;
pub mod gemini;
pub mod gemini_audio;
pub mod keychain;
pub mod ollama;
pub mod openai;
pub mod provider;
pub mod templates;

pub use provider::{
    build_prompt, chunk_transcript, estimate_tokens, parse_summary, AiError, AiProvider,
    CostEstimate, ModelPricing, Result, SummaryDraft, SummaryTemplate,
};
pub use templates::{default_template_for, ALL_TEMPLATES};

use crate::models::AiProviderKind;

/// Build a boxed [`AiProvider`] for the given kind + model. Used by the
/// Integrate phase to construct the configured provider from settings.
///
/// `model` is optional — `None` uses each provider's default. Ollama is the
/// safe default (local, no key); the cloud providers will surface
/// [`AiError::MissingApiKey`] at summarize time if no key is stored.
pub fn build_provider(kind: AiProviderKind, model: Option<String>) -> Box<dyn AiProvider> {
    match kind {
        AiProviderKind::Ollama => match model {
            Some(m) => Box::new(ollama::OllamaProvider::with_model(m)),
            None => Box::new(ollama::OllamaProvider::default()),
        },
        AiProviderKind::OpenAi => match model {
            Some(m) => Box::new(openai::OpenAiProvider::new(m)),
            None => Box::new(openai::OpenAiProvider::default()),
        },
        AiProviderKind::Claude => match model {
            Some(m) => Box::new(claude::ClaudeProvider::new(m)),
            None => Box::new(claude::ClaudeProvider::default()),
        },
        AiProviderKind::Gemini => match model {
            Some(m) => Box::new(gemini::GeminiProvider::new(m)),
            None => Box::new(gemini::GeminiProvider::default()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_dispatches_on_kind() {
        assert_eq!(
            build_provider(AiProviderKind::Ollama, None).kind(),
            AiProviderKind::Ollama
        );
        assert_eq!(
            build_provider(AiProviderKind::OpenAi, None).kind(),
            AiProviderKind::OpenAi
        );
        assert_eq!(
            build_provider(AiProviderKind::Claude, None).kind(),
            AiProviderKind::Claude
        );
        assert_eq!(
            build_provider(AiProviderKind::Gemini, None).kind(),
            AiProviderKind::Gemini
        );
    }

    #[test]
    fn build_provider_honours_custom_model() {
        let p = build_provider(AiProviderKind::Ollama, Some("custom-llm".into()));
        assert_eq!(p.active_model(), "custom-llm");
    }

    #[test]
    fn default_provider_is_local_ollama() {
        // The PRD makes Ollama the default; confirm the default kind builds a
        // local, no-cost provider.
        let p = build_provider(AiProviderKind::default(), None);
        assert_eq!(p.kind(), AiProviderKind::Ollama);
        assert!(!p.kind().is_cloud());
    }
}
