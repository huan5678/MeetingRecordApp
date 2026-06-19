//! Thin wrapper over the OS keychain (`keyring` crate) for cloud-provider API
//! keys. Keys are NEVER stored in plain text or in the SQLite DB (PRD §7.2);
//! they live in the OS keychain keyed by provider.
//!
//! The cloud providers ([`crate::ai::openai`], [`crate::ai::claude`],
//! [`crate::ai::gemini`]) call [`get_api_key`] lazily, just before a request,
//! so the key only sits in memory for the duration of the call.

use crate::models::AiProviderKind;

/// The keychain *service* name under which all of this app's secrets live.
pub const SERVICE: &str = "MeetingRecordApp";

/// The keychain *account* (entry name) for a given cloud provider's API key.
/// Ollama is local and has no key.
pub fn account_for(kind: AiProviderKind) -> Option<&'static str> {
    match kind {
        AiProviderKind::OpenAi => Some("openai_api_key"),
        AiProviderKind::Claude => Some("claude_api_key"),
        AiProviderKind::Gemini => Some("gemini_api_key"),
        AiProviderKind::Ollama => None,
    }
}

/// Read a provider's API key from the OS keychain. Returns `Ok(None)` when no
/// key has been stored yet (distinct from a keychain access error).
pub fn get_api_key(kind: AiProviderKind) -> Result<Option<String>, keyring::Error> {
    let Some(account) = account_for(kind) else {
        return Ok(None);
    };
    let entry = keyring::Entry::new(SERVICE, account)?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Store a provider's API key in the OS keychain (used by the settings command
/// surface during Integrate; provided here so the keychain key naming lives in
/// one place).
pub fn set_api_key(kind: AiProviderKind, secret: &str) -> Result<(), keyring::Error> {
    let Some(account) = account_for(kind) else {
        return Ok(());
    };
    let entry = keyring::Entry::new(SERVICE, account)?;
    entry.set_password(secret)
}

/// Remove a provider's stored API key. No-op (Ok) if there was none.
pub fn delete_api_key(kind: AiProviderKind) -> Result<(), keyring::Error> {
    let Some(account) = account_for(kind) else {
        return Ok(());
    };
    let entry = keyring::Entry::new(SERVICE, account)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_providers_have_distinct_accounts() {
        let openai = account_for(AiProviderKind::OpenAi).unwrap();
        let claude = account_for(AiProviderKind::Claude).unwrap();
        let gemini = account_for(AiProviderKind::Gemini).unwrap();
        assert_ne!(openai, claude);
        assert_ne!(claude, gemini);
        assert_ne!(openai, gemini);
    }

    #[test]
    fn ollama_has_no_keychain_account() {
        assert_eq!(account_for(AiProviderKind::Ollama), None);
    }
}
