//! OllamaProvider — local Ollama API client.
//!
//! Calls `http://localhost:11434/api/generate` for completions.
//! Ollama is auto-detected at startup; if unavailable, the provider
//! reports `healthy: false` and the router skips it.

use super::types::{CommandCandidate, CompletionCandidate, ErrorExplanation, TerminalContext};
use super::AiProvider;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Local Ollama AI provider.
pub struct OllamaProvider {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url,
            model,
            client,
        }
    }

    /// Call Ollama's generate endpoint and return the response text.
    async fn generate(&self, system_prompt: &str, user_prompt: &str) -> Result<String, String> {
        let url = format!("{}/api/generate", self.base_url.trim_end_matches('/'));
        let full_prompt = format!("{}\n\n{}", system_prompt, user_prompt);
        let body = OllamaRequest {
            model: self.model.clone(),
            prompt: full_prompt,
            stream: false,
        };

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {}: {}", status, text));
        }

        let ollama_resp: OllamaResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse Ollama response: {}", e))?;

        Ok(ollama_resp.response)
    }

    fn build_context_string(context: &TerminalContext) -> String {
        super::context::build_context_string(context)
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    async fn generate_command(
        &self,
        prompt: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CommandCandidate>, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a terminal assistant. Convert the user's natural language request into shell commands. Respond with JSON array of objects with fields: command, explanation, confidence. Keep commands safe and common.";
        let user = format!("Context:\n{}\n\nRequest: {}", ctx, prompt);
        let response = self.generate(system, &user).await?;
        super::cloud::parse_command_candidates_public(&response)
    }

    async fn explain_error(
        &self,
        error_output: &str,
        context: &TerminalContext,
    ) -> Result<ErrorExplanation, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a terminal error analyst. Analyze the error output and explain the root cause and suggest a fix. Respond with JSON object: {error_type, root_cause, suggestion, fix_command}.";
        let user = format!("Context:\n{}\n\nError output:\n{}", ctx, error_output);
        let response = self.generate(system, &user).await?;
        super::cloud::parse_error_explanation_public(&response)
    }

    async fn complete(
        &self,
        partial: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CompletionCandidate>, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a shell completion engine. Given a partial command, suggest completions. Respond with JSON array of objects: {text, description, confidence}.";
        let user = format!("Context:\n{}\n\nPartial: {}", ctx, partial);
        let response = self.generate(system, &user).await?;
        super::cloud::parse_completion_candidates_public(&response)
    }

    async fn health_check(&self) -> Result<bool, String> {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }

    fn provider_id(&self) -> &str {
        "ollama"
    }

    fn available_models(&self) -> Vec<String> {
        vec![
            "llama3.2".into(),
            "qwen2.5".into(),
            "mistral".into(),
            "phi3".into(),
        ]
    }
}

// ── Ollama API types ───────────────────────────────────────────

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_provider_available_models() {
        let provider = OllamaProvider::new("http://localhost:11434".into(), "llama3.2".into());
        let models = provider.available_models();
        assert!(models.contains(&"llama3.2".to_string()));
        assert!(models.contains(&"qwen2.5".to_string()));
    }

    #[test]
    fn ollama_provider_id() {
        let provider = OllamaProvider::new("http://localhost:11434".into(), "llama3.2".into());
        assert_eq!(provider.provider_id(), "ollama");
    }

    // ── Construction & configuration tests ─────────────────────────

    #[test]
    fn ollama_provider_new_preserves_base_url() {
        let provider = OllamaProvider::new("http://127.0.0.1:11434".into(), "mistral".into());
        assert_eq!(provider.base_url, "http://127.0.0.1:11434");
    }

    #[test]
    fn ollama_provider_new_preserves_model() {
        let provider = OllamaProvider::new("http://localhost:11434".into(), "phi3".into());
        assert_eq!(provider.model, "phi3");
    }

    #[test]
    fn ollama_provider_new_with_trailing_slash() {
        // Trailing slash in base_url should be handled by trim_end_matches in
        // generate(); constructor itself preserves the input.
        let provider = OllamaProvider::new("http://localhost:11434/".into(), "llama3.2".into());
        assert_eq!(provider.base_url, "http://localhost:11434/");
    }

    #[test]
    fn ollama_provider_available_models_complete_list() {
        let provider = OllamaProvider::new("http://localhost:11434".into(), "llama3.2".into());
        let models = provider.available_models();
        assert_eq!(models.len(), 4, "expected 4 default Ollama models");
        assert!(models.contains(&"llama3.2".to_string()));
        assert!(models.contains(&"qwen2.5".to_string()));
        assert!(models.contains(&"mistral".to_string()));
        assert!(models.contains(&"phi3".to_string()));
    }

    // ── Health check tests (against unreachable host) ──────────────
    //
    // These tests verify the health_check behavior without requiring a
    // running Ollama instance. We point the provider at a port that's
    // almost certainly closed.

    #[tokio::test]
    async fn ollama_health_check_returns_false_for_unreachable_host() {
        // Port 1 is reserved and typically not listening — connection refused.
        let provider = OllamaProvider::new("http://127.0.0.1:1".into(), "llama3.2".into());
        let healthy = provider.health_check().await.expect("health check");
        assert!(
            !healthy,
            "unreachable Ollama should report unhealthy, got healthy={}",
            healthy
        );
    }

    #[tokio::test]
    async fn ollama_health_check_returns_ok_result_for_closed_port() {
        // health_check should never return Err even when the connection fails;
        // it should return Ok(false).
        let provider = OllamaProvider::new("http://127.0.0.1:1".into(), "llama3.2".into());
        let result = provider.health_check().await;
        assert!(
            result.is_ok(),
            "health_check should return Ok even on connection failure"
        );
    }

    // ── build_context_string tests ─────────────────────────────────
    //
    // build_context_string is private, but we can test it indirectly via
    // the public interface. However, since it's a pure function with no
    // side effects, we expose it here for direct testing.

    #[test]
    fn ollama_build_context_string_empty_context() {
        let ctx = TerminalContext::default();
        let s = OllamaProvider::build_context_string(&ctx);
        assert!(s.is_empty(), "empty context should produce empty string");
    }

    #[test]
    fn ollama_build_context_string_all_fields() {
        let ctx = TerminalContext {
            cwd: Some("/root".into()),
            shell: Some("bash".into()),
            os: Some("linux".into()),
            recent_commands: vec!["ls".into(), "pwd".into()],
            last_exit_code: Some(0),
            recent_output: None,
        };
        let s = OllamaProvider::build_context_string(&ctx);
        assert!(s.contains("cwd: /root"));
        assert!(s.contains("shell: bash"));
        assert!(s.contains("os: linux"));
        assert!(s.contains("ls, pwd"));
        assert!(s.contains("last exit code: 0"));
    }

    #[test]
    fn ollama_build_context_string_excludes_recent_output() {
        let ctx = TerminalContext {
            cwd: Some("/tmp".into()),
            recent_output: Some("verbose log".into()),
            ..Default::default()
        };
        let s = OllamaProvider::build_context_string(&ctx);
        assert!(
            !s.contains("verbose log"),
            "recent_output should not be in context string"
        );
    }

    #[test]
    fn ollama_build_context_string_only_os() {
        let ctx = TerminalContext {
            os: Some("macos".into()),
            ..Default::default()
        };
        let s = OllamaProvider::build_context_string(&ctx);
        assert_eq!(s, "os: macos");
    }
}
