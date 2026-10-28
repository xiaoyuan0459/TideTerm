//! OneKey accounts shared by the Qt manager and SSH connection path.

use std::sync::{Mutex, OnceLock};
use tiderm_core::onekey::{OneKeyAccount, OneKeyAuthMethod, OneKeyStore};
use tiderm_core::protocol::ssh::SshAuth;

use crate::bridge::onekey::onekey_ffi::OneKeyRow;

fn store() -> &'static Mutex<OneKeyStore> {
    static STORE: OnceLock<Mutex<OneKeyStore>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(OneKeyStore::load()))
}

pub fn onekey_rows() -> Vec<OneKeyRow> {
    store()
        .lock()
        .unwrap()
        .list()
        .iter()
        .map(|a| OneKeyRow {
            id: a.id.clone(),
            name: a.name.clone(),
            username: a.username.clone(),
            kind: match a.auth_method {
                OneKeyAuthMethod::Password { .. } => 0,
                OneKeyAuthMethod::PrivateKey { .. } => 1,
                OneKeyAuthMethod::Agent => 2,
            },
            key_path: match &a.auth_method {
                OneKeyAuthMethod::PrivateKey {
                    private_key_path, ..
                } => private_key_path.clone(),
                _ => String::new(),
            },
        })
        .collect()
}

pub fn onekey_save(
    id: &str,
    name: &str,
    username: &str,
    kind: u8,
    secret: &str,
    key_path: &str,
) -> String {
    if name.trim().is_empty() || username.trim().is_empty() {
        return String::new();
    }
    let id = if id.is_empty() {
        format!(
            "ok-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    } else {
        id.to_string()
    };
    let mut guard = store().lock().unwrap();
    let existing = guard.find(&id).cloned();
    let auth_method = match kind {
        0 => {
            let password = if secret.is_empty() {
                match existing.as_ref().map(|a| &a.auth_method) {
                    Some(OneKeyAuthMethod::Password { password }) => password.clone(),
                    _ => return String::new(),
                }
            } else {
                secret.into()
            };
            OneKeyAuthMethod::Password { password }
        }
        1 if !key_path.trim().is_empty() => OneKeyAuthMethod::PrivateKey {
            private_key_path: key_path.into(),
            passphrase: if secret.is_empty() {
                match existing.as_ref().map(|a| &a.auth_method) {
                    Some(OneKeyAuthMethod::PrivateKey { passphrase, .. }) => passphrase.clone(),
                    _ => None,
                }
            } else {
                Some(secret.into())
            },
        },
        _ => return String::new(),
    };
    guard.upsert(OneKeyAccount {
        id,
        name: name.trim().into(),
        username: username.trim().into(),
        auth_method,
    })
}

pub fn onekey_delete(id: &str) -> bool {
    store().lock().unwrap().remove(id)
}

pub fn resolve_auth(id: &str) -> Result<(String, SshAuth), String> {
    let guard = store().lock().unwrap();
    let account = guard
        .find(id)
        .ok_or_else(|| format!("OneKey 账户不存在：{id}"))?;
    let username = account.username.clone();
    let auth = match &account.auth_method {
        OneKeyAuthMethod::Password { password } if !password.is_empty() => SshAuth::Password {
            password: password.clone(),
        },
        OneKeyAuthMethod::Password { .. } => {
            return Err("OneKey 密码未保存在磁盘；请在 OneKeys 中重新输入密码".into())
        }
        OneKeyAuthMethod::PrivateKey {
            private_key_path,
            passphrase,
        } => SshAuth::PublicKey {
            private_key_path: private_key_path.clone(),
            passphrase: passphrase.clone(),
        },
        OneKeyAuthMethod::Agent => return Err("SSH Agent 认证尚不可用".into()),
    };
    Ok((username, auth))
}
