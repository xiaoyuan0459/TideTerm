//! CloudProvider — OpenAI-compatible API client.
//!
//! Calls `{base_url}/v1/chat/completions` with the configured model.
//! Supports streaming responses via `stream: true`.

use super::types::{CommandCandidate, CompletionCandidate, ErrorExplanation, TerminalContext};
use super::AiProvider;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Cloud AI provider using OpenAI-compatible chat completions API.
pub struct CloudProvider {
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) api_key: Option<String>,
    client: reqwest::Client,
}

impl CloudProvider {
    pub fn new(base_url: String, model: String, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            base_url,
            model,
            api_key,
            client,
        }
    }

    /// Build a chat completion request and return the response text.
    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, String> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| "API key not configured".to_string())?;

        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/')
        );
        let body = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: system_prompt.into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user_prompt.into(),
                },
            ],
            temperature: 0.3,
            stream: false,
        };

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("API returned {}: {}", status, text));
        }

        let chat_resp: ChatResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse response: {}", e))?;

        chat_resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| "empty response from API".into())
    }

    /// Build the context string for the AI prompt.
    ///
    /// Delegates to the shared [`crate::ai::context::build_context_string`]
    /// (P2-R2: was duplicated verbatim in cloud.rs / ollama.rs).
    pub(crate) fn build_context_string(context: &TerminalContext) -> String {
        super::context::build_context_string(context)
    }
}

#[async_trait]
impl AiProvider for CloudProvider {
    async fn generate_command(
        &self,
        prompt: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CommandCandidate>, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a terminal assistant. Convert the user's natural language request into shell commands. Respond with JSON array of objects with fields: command, explanation, confidence. Keep commands safe and common.";
        let user = format!("Context:\n{}\n\nRequest: {}", ctx, prompt);
        let response = self.chat(system, &user).await?;
        parse_command_candidates_public(&response)
    }

    async fn explain_error(
        &self,
        error_output: &str,
        context: &TerminalContext,
    ) -> Result<ErrorExplanation, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a terminal error analyst. Analyze the error output and explain the root cause and suggest a fix. Respond with JSON object: {error_type, root_cause, suggestion, fix_command}.";
        let user = format!("Context:\n{}\n\nError output:\n{}", ctx, error_output);
        let response = self.chat(system, &user).await?;
        parse_error_explanation_public(&response)
    }

    async fn complete(
        &self,
        partial: &str,
        context: &TerminalContext,
    ) -> Result<Vec<CompletionCandidate>, String> {
        let ctx = Self::build_context_string(context);
        let system = "You are a shell completion engine. Given a partial command, suggest completions. Respond with JSON array of objects: {text, description, confidence}.";
        let user = format!("Context:\n{}\n\nPartial: {}", ctx, partial);
        let response = self.chat(system, &user).await?;
        parse_completion_candidates_public(&response)
    }

    async fn health_check(&self) -> Result<bool, String> {
        if self.api_key.is_none() {
            return Ok(false);
        }
        // Lightweight check: just verify the API key is set.
        // A full health check would call /v1/models, but that adds latency.
        Ok(true)
    }

    fn provider_id(&self) -> &str {
        "cloud"
    }

    fn available_models(&self) -> Vec<String> {
        vec![
            "gpt-4o-mini".into(),
            "gpt-4o".into(),
            "claude-3-5-sonnet".into(),
            "deepseek-chat".into(),
        ]
    }
}

// ── OpenAI-compatible API types ────────────────────────────────

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f64,
    stream: bool,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

// ── Response parsing helpers ───────────────────────────────────
//
// These are `pub` so the Ollama provider can reuse them.

/// Parse JSON array of command candidates from the AI response.
/// Tries to extract JSON from markdown code blocks first.
pub fn parse_command_candidates_public(response: &str) -> Result<Vec<CommandCandidate>, String> {
    let json_str = extract_json(response);
    serde_json::from_str::<Vec<CommandCandidate>>(&json_str).map_err(|e| {
        format!(
            "failed to parse command candidates: {} (input: {})",
            e, json_str
        )
    })
}

pub fn parse_error_explanation_public(response: &str) -> Result<ErrorExplanation, String> {
    let json_str = extract_json(response);
    serde_json::from_str::<ErrorExplanation>(&json_str).map_err(|e| {
        format!(
            "failed to parse error explanation: {} (input: {})",
            e, json_str
        )
    })
}

pub fn parse_completion_candidates_public(
    response: &str,
) -> Result<Vec<CompletionCandidate>, String> {
    let json_str = extract_json(response);
    serde_json::from_str::<Vec<CompletionCandidate>>(&json_str).map_err(|e| {
        format!(
            "failed to parse completion candidates: {} (input: {})",
            e, json_str
        )
    })
}

/// Extract JSON from a response that may be wrapped in markdown code blocks.
pub(crate) fn extract_json(response: &str) -> String {
    let trimmed = response.trim();
    // Try to find ```json ... ``` block
    if let Some(start) = trimmed.find("```json")
        && let Some(end) = trimmed[start + 7..].find("```")
    {
        return trimmed[start + 7..start + 7 + end].trim().to_string();
    }
    // Try to find ``` ... ``` block
    if let Some(start) = trimmed.find("```")
        && let Some(end) = trimmed[start + 3..].find("```")
    {
        return trimmed[start + 3..start + 3 + end].trim().to_string();
    }
    // Otherwise return as-is
    trimmed.to_string()
}
