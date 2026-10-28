//! SSH helper utilities — host-key type/fingerprint formatting and
//! keyboard-interactive challenge inference.

use ssh2::HostKeyType;
use std::sync::OnceLock;

/// Human-readable algorithm name for a libssh2 host key type (used in the
/// host-key-confirmation flow).
pub fn host_key_type_name(key_type: HostKeyType) -> &'static str {
    match key_type {
        HostKeyType::Rsa => "ssh-rsa",
        HostKeyType::Dss => "ssh-dss",
        HostKeyType::Ecdsa256 => "ecdsa-sha2-nistp256",
        HostKeyType::Ecdsa384 => "ecdsa-sha2-nistp384",
        HostKeyType::Ecdsa521 => "ecdsa-sha2-nistp521",
        HostKeyType::Ed25519 => "ssh-ed25519",
        HostKeyType::Unknown => "unknown",
    }
}

/// Format a SHA-256 host-key hash as a colon-separated lowercase hex string
/// for display in the host-key confirmation dialog.
pub fn sha256_fingerprint(hash: Option<&[u8]>) -> String {
    let hash = match hash {
        Some(h) => h,
        None => return "<unavailable>".to_string(),
    };
    hash.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

// ── Keyboard-interactive challenge inference ────────────────────

static OTP_RE: OnceLock<regex::Regex> = OnceLock::new();
static USERNAME_RE: OnceLock<regex::Regex> = OnceLock::new();

fn otp_re() -> &'static regex::Regex {
    OTP_RE.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(otp|verification\s*code|token|2fa|mfa)\b|验证码")
            .expect("compile OTP regex")
    })
}

fn username_re() -> &'static regex::Regex {
    USERNAME_RE.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(user\s*name|username)\b|用户名|账户")
            .expect("compile username regex")
    })
}

/// Infer the UI field type for a keyboard-interactive challenge.
pub fn infer_challenge_field_type(prompt: &str, echo: bool) -> String {
    if !echo {
        return "password".into();
    }
    let lowered = prompt.to_lowercase();
    if otp_re().is_match(&lowered) {
        "otp".into()
    } else if username_re().is_match(&lowered) {
        "username".into()
    } else {
        "text".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_false_always_password() {
        assert_eq!(
            infer_challenge_field_type("Verification code:", false),
            "password"
        );
    }

    #[test]
    fn echo_true_otp_inference() {
        assert_eq!(infer_challenge_field_type("Enter OTP:", true), "otp");
        assert_eq!(
            infer_challenge_field_type("Verification code:", true),
            "otp"
        );
    }

    #[test]
    fn echo_true_username_inference() {
        assert_eq!(infer_challenge_field_type("Username:", true), "username");
        assert_eq!(
            infer_challenge_field_type("请输入用户名:", true),
            "username"
        );
    }

    #[test]
    fn echo_true_text_fallback() {
        assert_eq!(infer_challenge_field_type("Favorite color:", true), "text");
    }

    #[test]
    fn host_key_type_names() {
        assert_eq!(host_key_type_name(HostKeyType::Rsa), "ssh-rsa");
        assert_eq!(host_key_type_name(HostKeyType::Ed25519), "ssh-ed25519");
        assert_eq!(host_key_type_name(HostKeyType::Unknown), "unknown");
    }

    #[test]
    fn sha256_fingerprint_hex_colon() {
        assert_eq!(
            sha256_fingerprint(Some(&[0xde, 0xad, 0xbe, 0xef])),
            "de:ad:be:ef"
        );
        assert_eq!(sha256_fingerprint(None), "<unavailable>");
    }
}
