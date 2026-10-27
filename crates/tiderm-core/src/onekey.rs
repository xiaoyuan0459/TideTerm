//! OneKey authentication — stored account credentials for quick SSH login (de-shelled
//! from `src-tauri/onekey.rs`). Passwords and passphrases are `#[serde(skip)]` — they
//! live only in memory and are never written to disk.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Authentication method stored in a OneKey account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OneKeyAuthMethod {
    #[serde(rename = "password")]
    Password {
        #[serde(skip)]
        password: String,
    },
    #[serde(rename = "private_key")]
    PrivateKey {
        private_key_path: String,
        #[serde(skip)]
        passphrase: Option<String>,
    },
    #[serde(rename = "agent")]
    Agent,
}

/// A stored OneKey account for quick SSH authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneKeyAccount {
    pub id: String,
    pub name: String,
    pub username: String,
    pub auth_method: OneKeyAuthMethod,
}

/// Manual credentials resolved from a OneKey account or user input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneKeyManualAuth {
    pub username: String,
    #[serde(skip)]
    pub password: String,
    pub private_key_path: String,
    #[serde(skip)]
    pub passphrase: String,
}

/// Input to `resolve_onekey_auth` — either an account ID or manual credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OneKeyAuthResult {
    #[serde(rename = "account")]
    Account { account_id: String },
    #[serde(rename = "manual")]
    Manual { manual: OneKeyManualAuth },
}

/// A file-backed store for OneKey accounts (stands in for `AppConfig.onekey_accounts`
/// until the full config store is de-shelled into this crate).
#[derive(Debug, Default)]
pub struct OneKeyStore {
    accounts: Vec<OneKeyAccount>,
    path: Option<PathBuf>,
}

impl OneKeyStore {
    /// Load the store from the default config directory.
    pub fn load() -> OneKeyStore {
        let path = crate::config_dir()
            .ok()
            .map(|d| d.join("onekey_accounts.json"));
        let accounts = match &path {
            Some(p) if p.exists() => fs::read_to_string(p)
                .ok()
                .and_then(|d| serde_json::from_str(&d).ok())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        OneKeyStore { accounts, path }
    }

    /// List all stored accounts.
    pub fn list(&self) -> &[OneKeyAccount] {
        &self.accounts
    }

    /// Insert or update an account (upsert by ID). Returns the ID.
    pub fn upsert(&mut self, account: OneKeyAccount) -> String {
        let id = account.id.clone();
        if let Some(existing) = self.accounts.iter_mut().find(|a| a.id == account.id) {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
        self.persist();
        id
    }

    /// Remove an account by ID. Returns `true` if removed.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != id);
        let removed = self.accounts.len() < before;
        if removed {
            self.persist();
        }
        removed
    }

    /// Find an account by ID.
    pub fn find(&self, id: &str) -> Option<&OneKeyAccount> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// Resolve an auth result into concrete credentials.
    pub fn resolve(&self, result: OneKeyAuthResult) -> Result<OneKeyManualAuth, String> {
        match result {
            OneKeyAuthResult::Account { account_id } => {
                let account = self
                    .find(&account_id)
                    .ok_or_else(|| format!("OneKey account not found: {account_id}"))?;
                resolve_account_to_manual(account)
            }
            OneKeyAuthResult::Manual { manual } => Ok(manual),
        }
    }

    fn persist(&self) {
        if let Some(path) = &self.path
            && let Ok(data) = serde_json::to_string(&self.accounts)
        {
            let _ = fs::write(path, data.as_bytes());
        }
    }
}

/// Convert a `OneKeyAccount` into `OneKeyManualAuth` (shared with the Tauri app's
/// `AppConfig`-backed account list).
pub fn resolve_account_to_manual(account: &OneKeyAccount) -> Result<OneKeyManualAuth, String> {
    match &account.auth_method {
        OneKeyAuthMethod::Password { password } => Ok(OneKeyManualAuth {
            username: account.username.clone(),
            password: password.clone(),
            private_key_path: String::new(),
            passphrase: String::new(),
        }),
        OneKeyAuthMethod::PrivateKey {
            private_key_path,
            passphrase,
        } => Ok(OneKeyManualAuth {
            username: account.username.clone(),
            password: String::new(),
            private_key_path: private_key_path.clone(),
            passphrase: passphrase.clone().unwrap_or_default(),
        }),
        OneKeyAuthMethod::Agent => Err("SSH Agent authentication is not yet supported".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_password_account(id: &str, name: &str, password: &str) -> OneKeyAccount {
        OneKeyAccount {
            id: id.into(),
            name: name.into(),
            username: "deploy".into(),
            auth_method: OneKeyAuthMethod::Password {
                password: password.into(),
            },
        }
    }

    #[test]
    fn upsert_inserts_and_updates() {
        let mut store = OneKeyStore::default();
        store.upsert(make_password_account("ok-1", "Prod", "secret"));
        assert_eq!(store.list().len(), 1);
        store.upsert(make_password_account("ok-1", "Updated", "pw"));
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].name, "Updated");
    }

    #[test]
    fn remove_returns_bool() {
        let mut store = OneKeyStore::default();
        store.upsert(make_password_account("ok-1", "Prod", "secret"));
        assert!(store.remove("ok-1"));
        assert!(!store.remove("nonexistent"));
    }

    #[test]
    fn resolve_account_password() {
        let mut store = OneKeyStore::default();
        store.upsert(make_password_account("ok-1", "Prod", "super-secret"));
        let manual = store
            .resolve(OneKeyAuthResult::Account {
                account_id: "ok-1".into(),
            })
            .expect("resolve");
        assert_eq!(manual.username, "deploy");
        assert_eq!(manual.password, "super-secret");
    }

    #[test]
    fn password_not_persisted() {
        let account = make_password_account("ok-1", "Prod", "leak-me");
        let json = serde_json::to_string(&account).expect("serialize");
        assert!(!json.contains("leak-me"));
        assert!(!json.contains("\"password\":"));
    }
}
