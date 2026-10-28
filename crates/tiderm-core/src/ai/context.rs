//! Shared AI prompt context helpers (P2-R2).
//!
//! `build_context_string` was previously duplicated verbatim in
//! `cloud.rs` and `ollama.rs`; both providers now delegate here so the
//! prompt-context format stays consistent across backends.

use super::types::TerminalContext;

/// Build the context string for the AI prompt.
///
/// Renders the terminal context (cwd / shell / os / recent commands /
/// last exit code) into a single prompt block shared by all providers.
pub fn build_context_string(context: &TerminalContext) -> String {
    let mut parts = Vec::new();
    if let Some(cwd) = &context.cwd {
        parts.push(format!("cwd: {}", cwd));
    }
    if let Some(shell) = &context.shell {
        parts.push(format!("shell: {}", shell));
    }
    if let Some(os) = &context.os {
        parts.push(format!("os: {}", os));
    }
    if !context.recent_commands.is_empty() {
        parts.push(format!(
            "recent commands: {}",
            context.recent_commands.join(", ")
        ));
    }
    if let Some(code) = context.last_exit_code {
        parts.push(format!("last exit code: {}", code));
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_produces_empty_string() {
        let ctx = TerminalContext::default();
        assert!(build_context_string(&ctx).is_empty());
    }

    #[test]
    fn all_fields_are_rendered() {
        let ctx = TerminalContext {
            cwd: Some("/home/user".into()),
            shell: Some("bash".into()),
            os: Some("linux".into()),
            recent_commands: vec!["ls".into(), "cd /".into()],
            last_exit_code: Some(1),
            recent_output: None,
        };
        let s = build_context_string(&ctx);
        assert!(s.contains("cwd: /home/user"));
        assert!(s.contains("shell: bash"));
        assert!(s.contains("os: linux"));
        assert!(s.contains("ls, cd /"));
        assert!(s.contains("last exit code: 1"));
    }

    #[test]
    fn recent_output_is_excluded() {
        let ctx = TerminalContext {
            cwd: Some("/tmp".into()),
            recent_output: Some("verbose log".into()),
            ..Default::default()
        };
        let s = build_context_string(&ctx);
        assert!(
            !s.contains("verbose log"),
            "recent_output should not be in context string"
        );
    }

    #[test]
    fn only_os_renders_single_line() {
        let ctx = TerminalContext {
            os: Some("macos".into()),
            ..Default::default()
        };
        assert_eq!(build_context_string(&ctx), "os: macos");
    }
}
