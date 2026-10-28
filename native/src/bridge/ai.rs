//! ai 域桥（ADR-014 按域拆桥第五域，Phase C ⑥）。
//!
//! 配置读写 + 异步生成（结果经 `ai_take_replies` 轮询领取）。`ai_` 前缀
//! 跨桥全局唯一；只传 POD。API key 只进内存（持久化侧 skip_serializing）。

pub use crate::rust::ai::{
    config_load, config_save, explain_error, generate_command, health_check, set_api_key,
    take_replies,
};

#[cxx::bridge]
pub mod ai_ffi {
    /// AI 配置（api_key 仅运行期；持久化侧不落盘）。
    pub struct AiCfgPod {
        /// cloud / ollama / auto。
        pub provider: String,
        pub base_url: String,
        pub model: String,
        pub privacy_mode: bool,
        /// 永不回填（读侧恒空；写入 = 设置运行期 key）。
        pub api_key: String,
    }

    /// 一条生成结果（kind：0=命令候选 1=错误解释 2=健康检查）。
    pub struct AiReplyPod {
        pub kind: u8,
        pub prompt: String,
        pub ok: bool,
        pub explanation: String,
        /// 候选命令（kind=0）。
        pub commands: Vec<String>,
        /// 候选解释（与 commands 平行）。
        pub notes: Vec<String>,
        /// 候选置信度（与 commands 平行，0–100）。
        pub confidences: Vec<u8>,
    }

    extern "Rust" {
        /// 读持久配置（api_key 恒空——不回填）。
        fn ai_config_get() -> AiCfgPod;
        /// 保存配置 + 注入运行期 key（api_key 空 = 只存其余字段）。
        fn ai_config_set(cfg: &AiCfgPod);
        /// 自然语言 → 命令候选（异步；结果经 ai_take_replies）。
        fn ai_generate(prompt: &str);
        /// 解释错误文本（异步）。
        fn ai_explain(error_text: &str);
        /// provider 健康检查（异步，kind=2）。
        fn ai_health();
        /// 领取全部已完成结果（UI 泵轮询）。
        fn ai_take_replies() -> Vec<AiReplyPod>;
    }
}

use crate::rust::ai::ai_model;
use ai_ffi::{AiCfgPod, AiReplyPod};

/// 桥名包装（实现侧沿用 rust::ai 的领域命名）。
pub(crate) fn ai_generate(prompt: &str) {
    generate_command(prompt);
}

pub(crate) fn ai_explain(error_text: &str) {
    explain_error(error_text);
}

pub(crate) fn ai_health() {
    health_check();
}

pub(crate) fn ai_config_get() -> AiCfgPod {
    let c = config_load();
    AiCfgPod {
        provider: c.provider,
        base_url: c.base_url,
        model: c.model,
        privacy_mode: c.privacy_mode,
        api_key: String::new(), // 永不回填
    }
}

pub(crate) fn ai_config_set(cfg: &AiCfgPod) {
    config_save(&ai_model::AiConfig {
        provider: cfg.provider.clone(),
        base_url: cfg.base_url.clone(),
        model: cfg.model.clone(),
        privacy_mode: cfg.privacy_mode,
        api_key: String::new(), // 持久化不含 key
    });
    if !cfg.api_key.is_empty() {
        set_api_key(&cfg.api_key);
    }
}

pub(crate) fn ai_take_replies() -> Vec<AiReplyPod> {
    take_replies()
        .into_iter()
        .map(|r| AiReplyPod {
            kind: r.kind,
            prompt: r.prompt,
            ok: r.ok,
            explanation: r.explanation,
            commands: r.candidates.iter().map(|c| c.command.clone()).collect(),
            notes: r.candidates.iter().map(|c| c.explanation.clone()).collect(),
            confidences: r
                .candidates
                .iter()
                .map(|c| (c.confidence.clamp(0.0, 1.0) * 100.0) as u8)
                .collect(),
        })
        .collect()
}
