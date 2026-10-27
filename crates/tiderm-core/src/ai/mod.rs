//! AI module — provider trait, router, and implementations.
//!
//! Architecture (ADR-008 API-first):
//! - `AiProvider` trait defines the contract for all AI backends.
//! - `CloudProvider` calls OpenAI-compatible APIs (reqwest HTTP).
//! - `OllamaProvider` calls local Ollama (localhost:11434).
//! - `AiRouter` routes requests to the best available provider with
//!   automatic fallback (Cloud → Ollama) and privacy mode support.

pub mod cloud;
pub mod context;
pub mod ollama;
pub mod types;

#[cfg(test)]
mod router_tests;
#[cfg(test)]
mod tests;

pub use types::{
    AiConfig, CommandCandidate, CompletionCandidate, ErrorExplanation, ProviderStatus,
    SuggestionChunk, TerminalContext,
};

use async_trait::async_trait;
use std::time::Duration;
use tokio::time::timeout as tokio_timeout;

/// AI provider trait — all backends implement this interface.
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Convert natural language to shell commands.
    async fn generate_command(
        &self,
        prompt: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CommandCandidate>, String>;

    /// Explain a terminal error and suggest a fix.
    async fn explain_error(
        &self,
        error_output: &str,
        context: &TerminalContext,
    ) -> Result<ErrorExplanation, String>;

    /// Generate inline completions for a partial command.
    async fn complete(
        &self,
        partial: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CompletionCandidate>, String>;

    /// Check whether the provider is healthy and available.
    async fn health_check(&self) -> Result<bool, String>;

    /// Provider identifier ("cloud", "ollama").
    fn provider_id(&self) -> &str;

    /// List available models.
    fn available_models(&self) -> Vec<String>;
}

/// Router that dispatches AI requests to the best available provider.
///
/// Priority: `[Cloud, Ollama]` (or `[Ollama]` in privacy mode).
/// When the primary provider fails or times out, the router falls
/// back to the next provider.
pub struct AiRouter {
    providers: Vec<Box<dyn AiProvider>>,
    config: AiConfig,
}

impl AiRouter {
    /// Create a new router with the given config.
    pub fn new(config: AiConfig) -> Self {
        let mut providers: Vec<Box<dyn AiProvider>> = Vec::new();

        if !config.privacy_mode && config.provider != "ollama" {
            providers.push(Box::new(cloud::CloudProvider::new(
                config.base_url.clone(),
                config.model.clone(),
                config.api_key.clone(),
            )));
        }

        if config.provider != "cloud" || config.privacy_mode {
            providers.push(Box::new(ollama::OllamaProvider::new(
                "http://localhost:11434".into(),
                if config.privacy_mode {
                    config.model.clone()
                } else {
                    "llama3.2".into()
                },
            )));
        }

        Self { providers, config }
    }

    /// Return the provider priority list (highest first).
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.provider_id()).collect()
    }

    /// Return the current config.
    pub fn config(&self) -> &AiConfig {
        &self.config
    }

    /// Per-provider timeout for AI requests.
    const PROVIDER_TIMEOUT: Duration = Duration::from_secs(15);

    /// Wrap a provider call with a timeout to prevent one slow provider
    /// from blocking the entire fallback chain.
    async fn with_timeout<T, F: std::future::Future<Output = Result<T, String>>>(
        provider_id: &str,
        fut: F,
    ) -> Result<T, String> {
        match tokio_timeout(Self::PROVIDER_TIMEOUT, fut).await {
            Ok(result) => result,
            Err(_) => Err(format!(
                "{provider_id}: request timed out after {}s",
                Self::PROVIDER_TIMEOUT.as_secs()
            )),
        }
    }

    /// Generate commands, trying providers in priority order.
    pub async fn generate_command(
        &self,
        prompt: &str,
        context: &TerminalContext,
    ) -> Result<(Vec<CommandCandidate>, String), String> {
        if self.providers.is_empty() {
            return Err("no AI providers available".into());
        }
        let mut last_error = String::new();
        for provider in &self.providers {
            match Self::with_timeout(
                provider.provider_id(),
                provider.generate_command(prompt, context),
            )
            .await
            {
                Ok(result) => return Ok((result, provider.provider_id().to_string())),
                Err(e) => {
                    if !last_error.is_empty() {
                        last_error.push_str("; ");
                    }
                    last_error.push_str(&format!("{}: {}", provider.provider_id(), e));
                }
            }
        }
        Err(last_error)
    }

    /// Explain an error, trying providers in priority order.
    pub async fn explain_error(
        &self,
        error_output: &str,
        context: &TerminalContext,
    ) -> Result<(ErrorExplanation, String), String> {
        if self.providers.is_empty() {
            return Err("no AI providers available".into());
        }
        let mut last_error = String::new();
        for provider in &self.providers {
            match Self::with_timeout(
                provider.provider_id(),
                provider.explain_error(error_output, context),
            )
            .await
            {
                Ok(result) => return Ok((result, provider.provider_id().to_string())),
                Err(e) => {
                    if !last_error.is_empty() {
                        last_error.push_str("; ");
                    }
                    last_error.push_str(&format!("{}: {}", provider.provider_id(), e));
                }
            }
        }
        Err(last_error)
    }

    /// Generate completions, trying providers in priority order.
    pub async fn complete(
        &self,
        partial: &str,
        context: &TerminalContext,
    ) -> Result<(Vec<CompletionCandidate>, String), String> {
        if self.providers.is_empty() {
            return Err("no AI providers available".into());
        }
        let mut last_error = String::new();
        for provider in &self.providers {
            match Self::with_timeout(provider.provider_id(), provider.complete(partial, context))
                .await
            {
                Ok(result) => return Ok((result, provider.provider_id().to_string())),
                Err(e) => {
                    if !last_error.is_empty() {
                        last_error.push_str("; ");
                    }
                    last_error.push_str(&format!("{}: {}", provider.provider_id(), e));
                }
            }
        }
        Err(last_error)
    }

    /// Check health of all providers.
    pub async fn health_check_all(&self) -> Vec<(String, bool)> {
        let mut results = Vec::new();
        for provider in &self.providers {
            let healthy = provider.health_check().await.unwrap_or(false);
            results.push((provider.provider_id().to_string(), healthy));
        }
        results
    }
}
