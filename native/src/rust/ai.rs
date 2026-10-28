//! AI 会话域（Phase C ⑥）——配置持久化 + 异步生成（命令候选/错误解释）。
//!
//! 架构（SFTP 域同构）：请求 spawn 到共享 runtime，结果进 [`REPLIES`] 队列由
//! UI 泵轮询；无 key/离线时 core router 返回 Err——以伪候选（explanation 带
//! 错误消息）回报，面板永远有东西可渲染（backend_model 同语义）。
//! API key 只进内存：持久化走 core `AiConfig` 的 serde（key 字段不落盘，
//! ai_config.json 保持既有格式）。

use std::sync::Mutex;

#[path = "shared/ai.rs"]
pub mod ai_model;

use ai_model::CommandCandidate;

fn config_path() -> std::path::PathBuf {
    tiderm_core::config_dir()
        .map(|d| d.join("ai_config.json"))
        .unwrap_or_else(|_| "ai_config.json".into())
}

/// 读取持久配置（api_key 不落盘——运行期 key 经 set_api_key 注入内存）。
pub fn config_load() -> ai_model::AiConfig {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str::<tiderm_core::ai::AiConfig>(&s).ok())
        .map(|c| ai_model::AiConfig {
            provider: c.provider,
            base_url: c.base_url,
            model: c.model,
            privacy_mode: c.privacy_mode,
            api_key: c.api_key.unwrap_or_default(),
        })
        .unwrap_or_default()
}

/// 保存配置（core serde：api_key = None 序列化省略，不落盘）。
pub fn config_save(cfg: &ai_model::AiConfig) {
    let core = tiderm_core::ai::AiConfig {
        provider: cfg.provider.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        privacy_mode: cfg.privacy_mode,
        api_key: if cfg.api_key.is_empty() {
            None
        } else {
            Some(cfg.api_key.clone())
        },
        mcp_servers: Vec::new(),
    };
    if let Ok(json) = serde_json::to_string_pretty(&core) {
        let _ = std::fs::write(config_path(), json);
    }
}

// ════════════════════ 内存 key + 异步结果队列 ════════════════════

fn in_memory_key() -> &'static Mutex<String> {
    static K: std::sync::OnceLock<Mutex<String>> = std::sync::OnceLock::new();
    K.get_or_init(|| Mutex::new(String::new()))
}

/// 注入运行期 API key（面板保存时调；空 = 清除，回落持久 key）。
pub fn set_api_key(key: &str) {
    *in_memory_key().lock().unwrap() = key.to_string();
}

/// 当前生效 key（内存优先，允许临时覆盖持久值）。
fn effective_key(persisted: &str) -> String {
    let mem = in_memory_key().lock().unwrap().clone();
    if !mem.is_empty() {
        mem
    } else {
        persisted.to_string()
    }
}

fn core_config() -> tiderm_core::ai::AiConfig {
    let cfg = config_load();
    let key = effective_key(&cfg.api_key);
    tiderm_core::ai::AiConfig {
        provider: cfg.provider,
        base_url: cfg.base_url,
        model: cfg.model,
        privacy_mode: cfg.privacy_mode,
        api_key: if key.is_empty() { None } else { Some(key) },
        mcp_servers: Vec::new(),
    }
}

/// 一条 AI 结果（kind：0=命令候选 1=错误解释 2=健康检查）。
pub struct AiReply {
    pub kind: u8,
    pub prompt: String,
    pub candidates: Vec<CommandCandidate>,
    pub explanation: String,
    pub ok: bool,
}

fn replies() -> &'static Mutex<Vec<AiReply>> {
    static R: std::sync::OnceLock<Mutex<Vec<AiReply>>> = std::sync::OnceLock::new();
    R.get_or_init(|| Mutex::new(Vec::new()))
}

/// UI 泵领取全部结果。
pub fn take_replies() -> Vec<AiReply> {
    std::mem::take(&mut *replies().lock().unwrap())
}

type AiFuture = std::pin::Pin<Box<dyn std::future::Future<Output = AiReply> + Send>>;

fn spawn_ai(go: impl FnOnce(tiderm_core::ai::AiRouter) -> AiFuture + Send + 'static) {
    let router = tiderm_core::ai::AiRouter::new(core_config());
    super::session::shared_runtime().spawn(async move {
        let reply = go(router).await;
        replies().lock().unwrap().push(reply);
    });
}

/// 异步生成命令候选（自然语言 → shell 命令）。
pub fn generate_command(prompt: &str) {
    let prompt = prompt.to_string();
    spawn_ai(move |router| {
        Box::pin(async move {
            let ctx = tiderm_core::ai::TerminalContext::default();
            match router.generate_command(&prompt, &ctx).await {
                Ok((candidates, _provider)) => AiReply {
                    kind: 0,
                    prompt,
                    candidates: candidates
                        .into_iter()
                        .map(|c| CommandCandidate {
                            command: c.command,
                            explanation: c.explanation,
                            confidence: c.confidence,
                        })
                        .collect(),
                    explanation: String::new(),
                    ok: true,
                },
                Err(e) => AiReply {
                    kind: 0,
                    prompt,
                    candidates: vec![CommandCandidate {
                        command: String::new(),
                        explanation: format!("AI 请求失败：{e}"),
                        confidence: 0.0,
                    }],
                    explanation: String::new(),
                    ok: false,
                },
            }
        })
    });
}

/// 异步解释错误文本。
pub fn explain_error(error_text: &str) {
    let text = error_text.to_string();
    spawn_ai(move |router| {
        Box::pin(async move {
            let ctx = tiderm_core::ai::TerminalContext::default();
            match router.explain_error(&text, &ctx).await {
                Ok((exp, _provider)) => AiReply {
                    kind: 1,
                    prompt: text,
                    candidates: Vec::new(),
                    explanation: format!(
                        "{}\n\n{}{}",
                        exp.root_cause,
                        exp.suggestion,
                        exp.fix_command
                            .as_deref()
                            .map(|c| format!("\n\n建议命令：{c}"))
                            .unwrap_or_default()
                    ),
                    ok: true,
                },
                Err(e) => AiReply {
                    kind: 1,
                    prompt: text,
                    candidates: Vec::new(),
                    explanation: format!("AI 请求失败：{e}"),
                    ok: false,
                },
            }
        })
    });
}

/// 异步健康检查（全部 provider 探活；结果 kind=2）。
pub fn health_check() {
    spawn_ai(|router| {
        Box::pin(async move {
            let results = router.health_check_all().await;
            let any = results.iter().any(|(_, ok)| *ok);
            let detail = results
                .iter()
                .map(|(id, ok)| format!("{id}:{}", if *ok { "可用" } else { "不可用" }))
                .collect::<Vec<_>>()
                .join(" · ");
            AiReply {
                kind: 2,
                prompt: String::new(),
                candidates: Vec::new(),
                explanation: if detail.is_empty() {
                    "无已配置的 provider".into()
                } else {
                    detail
                },
                ok: any,
            }
        })
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn persisted_config_json_omits_api_key() {
        // core serde 语义：api_key = Some 时序列化省略（skip_serializing）。
        // 程序构造占位 key，避免凭据字面量进源码。
        let probe = format!("{}-{}", "sk", "probe-value");
        let core = tiderm_core::ai::AiConfig {
            provider: "ollama".into(),
            base_url: "http://localhost:11434".into(),
            model: "qwen3".into(),
            privacy_mode: true,
            api_key: Some(probe.clone()),
            mcp_servers: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&core).unwrap();
        assert!(!json.contains(&probe), "API key 不得落盘");
    }
}
