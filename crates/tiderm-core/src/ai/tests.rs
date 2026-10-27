//! Tests for the AI cloud module — parsing helpers and CloudProvider behavior.

use crate::ai::cloud::{
    extract_json, parse_command_candidates_public, parse_completion_candidates_public,
    parse_error_explanation_public, CloudProvider,
};
use crate::ai::types::TerminalContext;
use crate::ai::AiProvider;

// ── Cloud parsing tests ──────────────────────────────────────────

#[test]
fn extract_json_from_markdown_block() {
    let input = "Here is the result:\n```json\n[{\"command\":\"ls\"}]\n```\nDone.";
    assert_eq!(extract_json(input), "[{\"command\":\"ls\"}]");
}

#[test]
fn extract_json_from_plain_block() {
    let input = "```\n{\"key\":\"value\"}\n```";
    assert_eq!(extract_json(input), "{\"key\":\"value\"}");
}

#[test]
fn extract_json_plain_text() {
    let input = "[{\"command\":\"ls\",\"explanation\":\"list\",\"confidence\":1.0}]";
    assert_eq!(extract_json(input), input);
}

#[test]
fn parse_command_candidates_valid() {
    let json = r#"[{"command":"ls -la","explanation":"list all files","confidence":0.9}]"#;
    let result = parse_command_candidates_public(json).expect("parse");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].command, "ls -la");
    assert_eq!(result[0].confidence, 0.9);
}

#[test]
fn parse_error_explanation_valid() {
    let json = r#"{"error_type":"permission_denied","root_cause":"no write access","suggestion":"use sudo","fix_command":"sudo chmod 644 file"}"#;
    let result = parse_error_explanation_public(json).expect("parse");
    assert_eq!(result.error_type, "permission_denied");
    assert_eq!(result.fix_command.as_deref(), Some("sudo chmod 644 file"));
}

#[test]
fn build_context_string_includes_fields() {
    let ctx = TerminalContext {
        cwd: Some("/home/user".into()),
        shell: Some("bash".into()),
        os: Some("linux".into()),
        recent_commands: vec!["ls".into(), "cd /".into()],
        last_exit_code: Some(1),
        recent_output: None,
    };
    let s = CloudProvider::build_context_string(&ctx);
    assert!(s.contains("cwd: /home/user"));
    assert!(s.contains("shell: bash"));
    assert!(s.contains("os: linux"));
    assert!(s.contains("ls, cd /"));
    assert!(s.contains("last exit code: 1"));
}

#[test]
fn cloud_provider_available_models() {
    let provider = CloudProvider::new("https://api.openai.com".into(), "gpt-4o-mini".into(), None);
    let models = provider.available_models();
    assert!(models.contains(&"gpt-4o-mini".to_string()));
    assert!(models.contains(&"claude-3-5-sonnet".to_string()));
}

// ── Error scenario tests ───────────────────────────────────────

#[test]
fn parse_command_candidates_invalid_json() {
    let result = parse_command_candidates_public("not valid json");
    assert!(
        result.is_err(),
        "invalid JSON should return error, got: {:?}",
        result
    );
}

#[test]
fn parse_command_candidates_empty_array() {
    let result = parse_command_candidates_public("[]").expect("parse");
    assert!(
        result.is_empty(),
        "empty JSON array should produce empty vec"
    );
}

#[test]
fn parse_command_candidates_missing_field() {
    let json = r#"[{"command":"ls","confidence":0.5}]"#;
    let result = parse_command_candidates_public(json);
    assert!(
        result.is_err(),
        "missing required field should fail deserialization"
    );
}

#[test]
fn parse_error_explanation_missing_fix_command() {
    let json = r#"{"error_type":"x","root_cause":"y","suggestion":"z"}"#;
    let result = parse_error_explanation_public(json).expect("parse");
    assert_eq!(result.error_type, "x");
    assert_eq!(result.root_cause, "y");
    assert_eq!(result.suggestion, "z");
    assert!(
        result.fix_command.is_none(),
        "missing fix_command should default to None"
    );
}

#[test]
fn parse_error_explanation_invalid_json() {
    let result = parse_error_explanation_public("not json");
    assert!(result.is_err());
}

#[test]
fn parse_completion_candidates_valid() {
    let json = r#"[{"text":"ls -la","description":"list files","confidence":0.9}]"#;
    let result = parse_completion_candidates_public(json).expect("parse");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].text, "ls -la");
    assert_eq!(result[0].description.as_deref(), Some("list files"));
    assert_eq!(result[0].confidence, 0.9);
}

#[test]
fn parse_completion_candidates_missing_description() {
    let json = r#"[{"text":"cd","confidence":0.5}]"#;
    let result = parse_completion_candidates_public(json).expect("parse");
    assert_eq!(result.len(), 1);
    assert!(result[0].description.is_none());
}

#[test]
fn parse_completion_candidates_invalid_json() {
    let result = parse_completion_candidates_public("not json");
    assert!(result.is_err());
}

#[test]
fn extract_json_with_markdown_block_no_closing() {
    let input = "```json\n[{\"command\":\"ls\"}]";
    let result = extract_json(input);
    assert_eq!(result, input.trim());
}

#[test]
fn extract_json_with_nested_braces_in_text() {
    let input = r#"{"key":{"nested":"value"}}"#;
    let result = extract_json(input);
    assert_eq!(result, input);
}

#[test]
fn extract_json_empty_string() {
    let result = extract_json("");
    assert_eq!(result, "");
}

#[test]
fn extract_json_only_whitespace() {
    let result = extract_json("   \n\t  ");
    assert_eq!(result, "");
}

// ── CloudProvider behavior tests ──────────────────────────────

#[test]
fn cloud_provider_new_with_none_api_key() {
    let provider = CloudProvider::new("https://api.openai.com".into(), "gpt-4o-mini".into(), None);
    assert!(provider.api_key.is_none());
}

#[test]
fn cloud_provider_new_with_api_key() {
    let provider = CloudProvider::new(
        "https://api.openai.com".into(),
        "gpt-4o-mini".into(),
        Some("sk-test".into()),
    );
    assert_eq!(provider.api_key.as_deref(), Some("sk-test"));
}

#[tokio::test]
async fn cloud_provider_health_check_no_api_key_returns_false() {
    let provider = CloudProvider::new("https://api.openai.com".into(), "gpt-4o-mini".into(), None);
    let healthy = provider.health_check().await.expect("health check");
    assert!(!healthy, "provider without API key should report unhealthy");
}

#[tokio::test]
async fn cloud_provider_health_check_with_api_key_returns_true() {
    let provider = CloudProvider::new(
        "https://api.openai.com".into(),
        "gpt-4o-mini".into(),
        Some("sk-test".into()),
    );
    let healthy = provider.health_check().await.expect("health check");
    assert!(
        healthy,
        "provider with API key should report healthy (lightweight check)"
    );
}

#[test]
fn cloud_provider_provider_id() {
    let provider = CloudProvider::new("https://api.openai.com".into(), "gpt-4o-mini".into(), None);
    assert_eq!(provider.provider_id(), "cloud");
}

#[test]
fn build_context_string_empty_context() {
    let ctx = TerminalContext::default();
    let s = CloudProvider::build_context_string(&ctx);
    assert!(
        s.is_empty(),
        "empty context should produce empty string, got: {}",
        s
    );
}

#[test]
fn build_context_string_only_cwd() {
    let ctx = TerminalContext {
        cwd: Some("/tmp".into()),
        ..Default::default()
    };
    let s = CloudProvider::build_context_string(&ctx);
    assert_eq!(s, "cwd: /tmp");
}

#[test]
fn build_context_string_only_exit_code() {
    let ctx = TerminalContext {
        last_exit_code: Some(127),
        ..Default::default()
    };
    let s = CloudProvider::build_context_string(&ctx);
    assert_eq!(s, "last exit code: 127");
}

#[test]
fn build_context_string_with_recent_output_is_ignored() {
    let ctx = TerminalContext {
        cwd: Some("/home".into()),
        recent_output: Some("huge log output".into()),
        ..Default::default()
    };
    let s = CloudProvider::build_context_string(&ctx);
    assert!(
        !s.contains("huge log output"),
        "recent_output should not be included in context string"
    );
    assert!(s.contains("cwd: /home"));
}
