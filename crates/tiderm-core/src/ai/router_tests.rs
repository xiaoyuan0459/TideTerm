//! Tests for the AI router — provider fallback, error aggregation, and construction.

use async_trait::async_trait;

use crate::ai::types::TerminalContext;
use crate::ai::{
    AiConfig, AiProvider, AiRouter, CommandCandidate, CompletionCandidate, ErrorExplanation,
};

struct MockProvider {
    id: &'static str,
    healthy: bool,
}

#[async_trait]
impl AiProvider for MockProvider {
    async fn generate_command(
        &self,
        prompt: &str,
        _context: &TerminalContext,
    ) -> Result<Vec<CommandCandidate>, String> {
        if !self.healthy {
            return Err("unhealthy".into());
        }
        Ok(vec![CommandCandidate {
            command: format!("echo '{}'", prompt),
            explanation: "mock".into(),
            confidence: 1.0,
        }])
    }

    async fn explain_error(
        &self,
        _output: &str,
        _context: &TerminalContext,
    ) -> Result<ErrorExplanation, String> {
        if !self.healthy {
            return Err("unhealthy".into());
        }
        Ok(ErrorExplanation {
            error_type: "mock".into(),
            root_cause: "mock".into(),
            suggestion: "mock".into(),
            fix_command: None,
        })
    }

    async fn complete(
        &self,
        _partial: &str,
        _context: &TerminalContext,
    ) -> Result<Vec<CompletionCandidate>, String> {
        if !self.healthy {
            return Err("unhealthy".into());
        }
        Ok(Vec::new())
    }

    async fn health_check(&self) -> Result<bool, String> {
        Ok(self.healthy)
    }

    fn provider_id(&self) -> &str {
        self.id
    }

    fn available_models(&self) -> Vec<String> {
        vec!["mock-model".into()]
    }
}

#[tokio::test]
async fn router_falls_back_to_next_provider() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: false,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: true,
            }),
        ],
        config: AiConfig::default(),
    };

    let (candidates, provider_id) = router
        .generate_command("test", &TerminalContext::default())
        .await
        .expect("should succeed via fallback");
    assert_eq!(provider_id, "ollama");
    assert_eq!(candidates[0].command, "echo 'test'");
}

#[tokio::test]
async fn router_returns_error_when_all_providers_fail() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: false,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: false,
            }),
        ],
        config: AiConfig::default(),
    };

    let result = router
        .generate_command("test", &TerminalContext::default())
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn router_health_check_reports_per_provider() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: true,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: false,
            }),
        ],
        config: AiConfig::default(),
    };

    let results = router.health_check_all().await;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], ("cloud".to_string(), true));
    assert_eq!(results[1], ("ollama".to_string(), false));
}

#[tokio::test]
async fn router_returns_error_when_no_providers() {
    let router = AiRouter {
        providers: Vec::new(),
        config: AiConfig::default(),
    };
    let result = router
        .generate_command("test", &TerminalContext::default())
        .await;
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err(),
        "no AI providers available",
        "empty router should return explicit error"
    );
}

#[tokio::test]
async fn router_explain_error_falls_back() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: false,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: true,
            }),
        ],
        config: AiConfig::default(),
    };
    let (explanation, provider_id) = router
        .explain_error("err", &TerminalContext::default())
        .await
        .expect("fallback should succeed");
    assert_eq!(provider_id, "ollama");
    assert_eq!(explanation.error_type, "mock");
}

#[tokio::test]
async fn router_explain_error_no_providers() {
    let router = AiRouter {
        providers: Vec::new(),
        config: AiConfig::default(),
    };
    let result = router
        .explain_error("err", &TerminalContext::default())
        .await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "no AI providers available");
}

#[tokio::test]
async fn router_complete_falls_back() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: false,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: true,
            }),
        ],
        config: AiConfig::default(),
    };
    let (_, provider_id) = router
        .complete("ls", &TerminalContext::default())
        .await
        .expect("fallback should succeed");
    assert_eq!(provider_id, "ollama");
}

#[tokio::test]
async fn router_complete_no_providers() {
    let router = AiRouter {
        providers: Vec::new(),
        config: AiConfig::default(),
    };
    let result = router.complete("ls", &TerminalContext::default()).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "no AI providers available");
}

#[tokio::test]
async fn router_health_check_empty_providers() {
    let router = AiRouter {
        providers: Vec::new(),
        config: AiConfig::default(),
    };
    let results = router.health_check_all().await;
    assert!(
        results.is_empty(),
        "empty router should report no providers"
    );
}

#[tokio::test]
async fn router_uses_first_healthy_provider() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: true,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: true,
            }),
        ],
        config: AiConfig::default(),
    };
    let (_, provider_id) = router
        .generate_command("test", &TerminalContext::default())
        .await
        .expect("should succeed");
    assert_eq!(
        provider_id, "cloud",
        "router should prefer the first healthy provider"
    );
}

#[tokio::test]
async fn router_aggregates_error_messages_when_all_fail() {
    let router = AiRouter {
        providers: vec![
            Box::new(MockProvider {
                id: "cloud",
                healthy: false,
            }),
            Box::new(MockProvider {
                id: "ollama",
                healthy: false,
            }),
        ],
        config: AiConfig::default(),
    };
    let err = router
        .generate_command("test", &TerminalContext::default())
        .await
        .unwrap_err();
    assert!(
        err.contains("cloud") && err.contains("ollama"),
        "error should mention all failing providers, got: {}",
        err
    );
}

// ── AiRouter::new construction tests ──────────────────────────

#[test]
fn router_new_privacy_mode_only_ollama() {
    let config = AiConfig {
        privacy_mode: true,
        provider: "auto".into(),
        ..Default::default()
    };
    let router = AiRouter::new(config);
    let ids = router.provider_ids();
    assert_eq!(
        ids,
        vec!["ollama"],
        "privacy mode must only include ollama provider"
    );
}

#[test]
fn router_new_provider_cloud_only() {
    let config = AiConfig {
        privacy_mode: false,
        provider: "cloud".into(),
        ..Default::default()
    };
    let router = AiRouter::new(config);
    let ids = router.provider_ids();
    assert_eq!(
        ids,
        vec!["cloud"],
        "provider=cloud must only include cloud provider"
    );
}

#[test]
fn router_new_provider_ollama_only() {
    let config = AiConfig {
        privacy_mode: false,
        provider: "ollama".into(),
        ..Default::default()
    };
    let router = AiRouter::new(config);
    let ids = router.provider_ids();
    assert_eq!(
        ids,
        vec!["ollama"],
        "provider=ollama must only include ollama provider"
    );
}

#[test]
fn router_new_provider_auto_includes_both() {
    let config = AiConfig {
        privacy_mode: false,
        provider: "auto".into(),
        ..Default::default()
    };
    let router = AiRouter::new(config);
    let ids = router.provider_ids();
    assert_eq!(
        ids,
        vec!["cloud", "ollama"],
        "provider=auto must include cloud first, then ollama"
    );
}

#[test]
fn router_config_returns_reference() {
    let config = AiConfig {
        model: "test-model".into(),
        ..Default::default()
    };
    let router = AiRouter::new(config);
    assert_eq!(router.config().model, "test-model");
}
