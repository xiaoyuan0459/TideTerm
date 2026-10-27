//! Master password and credential vault (de-shelled from `src-tauri/master_password.rs`).
//!
//! Optional master-password protection for stored credentials. When enabled, the user
//! sets a password that is used to derive an encryption key (Argon2id) which encrypts /
//! decrypts a credential vault file (AES-256-GCM). The `#[tauri::command]` wrappers stay
//! in the Tauri app; these are the pure vault operations.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{
    password_hash::{
        rand_core::RngCore, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
    },
    Argon2, Params,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

/// The vault file schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    kdf: KdfParams,
    verify_hash: String,
    credentials: HashMap<String, EncryptedBlob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KdfParams {
    salt: String,
    m: u32,
    t: u32,
    p: u32,
}

/// An AES-256-GCM encrypted credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedBlob {
    iv: String,
    ct: String,
}

/// Runtime override of the vault directory. Production always uses [`None`]
/// (the default config dir); the Qt shell's `vault_test_dir` self-test hook
/// redirects it so offscreen checks never touch the user's real vault.
static VAULT_DIR_OVERRIDE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Redirect the vault directory (self-test hermeticity); `None` restores the
/// default `config_dir()/vault.json` location.
pub fn set_vault_dir(dir: Option<PathBuf>) {
    *VAULT_DIR_OVERRIDE.lock().unwrap() = dir;
}

/// The vault file path.
fn vault_path() -> Result<PathBuf, String> {
    if let Some(dir) = VAULT_DIR_OVERRIDE.lock().unwrap().clone() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("create vault dir: {e}"))?;
        return Ok(dir.join("vault.json"));
    }
    Ok(crate::config_dir()?.join("vault.json"))
}

/// Derive a 32-byte key from the password and salt using Argon2id.
fn derive_key(password: &str, salt: &[u8], params: &Params) -> Result<Zeroizing<[u8; 32]>, String> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        params.clone(),
    );
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|e| format!("key derivation failed: {e}"))?;
    Ok(key)
}

fn encrypt(plaintext: &[u8], key: &[u8; 32]) -> Result<EncryptedBlob, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init: {e}"))?;
    let mut iv_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut iv_bytes);
    let nonce = Nonce::from_slice(&iv_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| format!("encrypt: {e}"))?;
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    Ok(EncryptedBlob {
        iv: engine.encode(iv_bytes),
        ct: engine.encode(ct),
    })
}

fn decrypt(blob: &EncryptedBlob, key: &[u8; 32]) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let iv_bytes = engine
        .decode(&blob.iv)
        .map_err(|e| format!("decode iv: {e}"))?;
    if iv_bytes.len() != 12 {
        return Err("invalid vault iv length".into());
    }
    let ct = engine
        .decode(&blob.ct)
        .map_err(|e| format!("decode ct: {e}"))?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = Nonce::from_slice(&iv_bytes);
    cipher
        .decrypt(nonce, ct.as_ref())
        .map_err(|e| format!("decrypt: {e}"))
}

/// Check whether a master password has been set.
pub fn is_set() -> bool {
    vault_path().ok().is_some_and(|p| p.exists())
}

/// Set the master password for the first time.
pub fn set_password(password: &str) -> Result<(), String> {
    if password.len() < 8 {
        return Err("password must be at least 8 characters".to_string());
    }
    if is_set() {
        return Err("master password already set".to_string());
    }
    let salt = SaltString::generate(&mut OsRng);
    let params = Params::new(65536, 3, 4, None).map_err(|e| format!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| format!("hash password: {e}"))?;

    let vault = VaultFile {
        version: 1,
        kdf: KdfParams {
            salt: salt.to_string(),
            m: 65536,
            t: 3,
            p: 4,
        },
        verify_hash: hash.to_string(),
        credentials: HashMap::new(),
    };
    let data = serde_json::to_string_pretty(&vault).map_err(|e| format!("serialize vault: {e}"))?;
    write_vault_file(&vault_path()?, data.as_bytes())?;
    Ok(())
}

/// Verify a password attempt (throttled client-side if needed). Returns `Ok(true)` on
/// success, `Ok(false)` on wrong password, `Err` on infrastructure errors.
///
/// 节流（S1，复刻旧 Tauri 后端 `throttle.rs`）：锁定期间直接拒绝、**不跑 Argon2**，
/// 让暴力破解的成本只剩时间。
pub fn verify_password(password: &str) -> Result<bool, String> {
    if let Some(remaining) = crate::throttle::throttle_remaining() {
        return Err(crate::throttle::lockout_error(remaining));
    }
    let path = vault_path()?;
    let vault = load_or_new(&path)?;

    let parsed = PasswordHash::new(&vault.verify_hash).map_err(|e| format!("parse hash: {e}"))?;
    let argon2 = Argon2::default();
    match argon2.verify_password(password.as_bytes(), &parsed) {
        Ok(()) => {
            crate::throttle::record_success();
            Ok(true)
        }
        Err(argon2::password_hash::Error::Password) => {
            crate::throttle::record_failure();
            Ok(false)
        }
        Err(e) => Err(format!("verify failed: {e}")),
    }
}

/// Encrypt a credential string under the master password and store it by `id`.
pub fn store_credential(id: &str, password: &str, plaintext: &str) -> Result<(), String> {
    let path = vault_path()?;
    let mut vault = load_or_new(&path)?;
    let key = derive_key_from_vault(password, &vault)?;
    let blob = encrypt(plaintext.as_bytes(), &key)?;
    vault.credentials.insert(id.to_string(), blob);
    save(&path, &vault)
}

/// Decrypt a stored credential by `id`.
pub fn load_credential(id: &str, password: &str) -> Result<String, String> {
    let path = vault_path()?;
    let vault = load_or_new(&path)?;
    let key = derive_key_from_vault(password, &vault)?;
    let blob = vault
        .credentials
        .get(id)
        .ok_or_else(|| format!("no credential: {id}"))?;
    let plain = decrypt(blob, &key)?;
    Ok(String::from_utf8_lossy_owned(plain))
}

/// Whether a credential is stored under `id` (no crypto, no unlock needed).
pub fn has_credential(id: &str) -> bool {
    vault_path().ok().is_some_and(|path| {
        load_or_new(&path)
            .ok()
            .is_some_and(|vault| vault.credentials.contains_key(id))
    })
}

/// Remove a credential by `id`. Returns `true` when an entry was removed.
///
/// Removal rewrites the vault without decrypting anything, so it needs no
/// master password: deleting a session must clean its vault entries even in a
/// locked shell.
pub fn remove_credential(id: &str) -> Result<bool, String> {
    let path = vault_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let mut vault = load_or_new(&path)?;
    if vault.credentials.remove(id).is_none() {
        return Ok(false);
    }
    save(&path, &vault).map(|_| true)
}

/// Change the master password, re-encrypting all stored credentials under the new key.
///
/// (B6) Ported from the app-side `master_password.rs`. Brute-force throttling and audit
/// logging stay in the Tauri command layer; this is the pure vault re-encryption.
pub fn change_password(old_password: &str, new_password: &str) -> Result<(), String> {
    // 节流（S1）：旧密码验证与 `verify_password` 共用同一暴力破解节流器，
    // 因此锁定同样覆盖改密路径（对齐 .rules/10-architecture.md）。
    if let Some(remaining) = crate::throttle::throttle_remaining() {
        return Err(crate::throttle::lockout_error(remaining));
    }
    if new_password.len() < 8 {
        return Err("password must be at least 8 characters".to_string());
    }
    let path = vault_path()?;
    let mut vault = load_or_new(&path)?;

    // Verify old password.
    let parsed = PasswordHash::new(&vault.verify_hash).map_err(|e| format!("parse hash: {e}"))?;
    let argon2 = Argon2::default();
    argon2
        .verify_password(old_password.as_bytes(), &parsed)
        .map_err(|_| {
            crate::throttle::record_failure();
            "old password incorrect".to_string()
        })?;
    crate::throttle::record_success();

    // Decrypt all credentials with the old key (Zeroizing so plaintext is zeroed on drop).
    let old_key = derive_key_from_vault(old_password, &vault)?;
    let mut plaintext_creds: HashMap<String, Zeroizing<Vec<u8>>> = HashMap::new();
    for (id, blob) in &vault.credentials {
        let pt = Zeroizing::new(decrypt(blob, &old_key)?);
        plaintext_creds.insert(id.clone(), pt);
    }

    // New salt + hash + key. The key derives from the DECODED raw salt bytes so it
    // matches `derive_key_from_vault` used by store/load_credential.
    let new_salt = SaltString::generate(&mut OsRng);
    let new_params = Params::new(65536, 3, 4, None).map_err(|e| format!("argon2 params: {e}"))?;
    let mut new_salt_buf = [0u8; 64];
    let new_salt_raw = new_salt
        .decode_b64(&mut new_salt_buf)
        .map_err(|e| format!("decode new salt: {e}"))?;
    let new_key = derive_key(new_password, new_salt_raw, &new_params)?;
    let new_argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        new_params,
    );
    let new_hash = new_argon2
        .hash_password(new_password.as_bytes(), &new_salt)
        .map_err(|e| format!("hash password: {e}"))?;

    // Re-encrypt all credentials with the new key.
    let mut new_creds = HashMap::new();
    for (id, pt) in &plaintext_creds {
        new_creds.insert(id.clone(), encrypt(pt, &new_key)?);
    }

    vault.kdf = KdfParams {
        salt: new_salt.to_string(),
        m: 65536,
        t: 3,
        p: 4,
    };
    vault.verify_hash = new_hash.to_string();
    vault.credentials = new_creds;
    save(&path, &vault)
}

fn derive_key_from_vault(password: &str, vault: &VaultFile) -> Result<Zeroizing<[u8; 32]>, String> {
    // Extract the salt from the stored verification hash (the same salt used for the
    // password hash), then base64-decode it back to raw bytes for key derivation.
    let parsed = PasswordHash::new(&vault.verify_hash).map_err(|e| format!("parse hash: {e}"))?;
    let salt_b64 = parsed
        .salt
        .ok_or_else(|| "vault salt missing".to_string())?
        .as_str()
        .to_string();
    use base64::Engine;
    // Argon2 PHC salts are base64 without padding.
    let raw_salt = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(salt_b64)
        .map_err(|e| format!("decode salt: {e}"))?;
    let params = Params::new(vault.kdf.m, vault.kdf.t, vault.kdf.p, None)
        .map_err(|e| format!("argon2 params: {e}"))?;
    derive_key(password, &raw_salt, &params)
}

fn load_or_new(path: &PathBuf) -> Result<VaultFile, String> {
    if path.exists() {
        let data = fs::read_to_string(path).map_err(|e| format!("read vault: {e}"))?;
        let vault: VaultFile =
            serde_json::from_str(&data).map_err(|e| format!("parse vault: {e}"))?;
        validate_vault(&vault)?;
        Ok(vault)
    } else {
        Err("no vault; set a master password first".to_string())
    }
}

fn validate_vault(vault: &VaultFile) -> Result<(), String> {
    if vault.version != 1 {
        return Err("unsupported vault version".into());
    }
    // Version 1 writers use 64 MiB, three passes and four lanes. Bound both
    // parameter sources: PasswordVerifier reads the PHC hash, not vault.kdf.
    let bounded = |m, t, p| m <= 65536 && t <= 3 && p <= 4;
    if !bounded(vault.kdf.m, vault.kdf.t, vault.kdf.p) {
        return Err("vault KDF exceeds supported limits".into());
    }
    Params::new(vault.kdf.m, vault.kdf.t, vault.kdf.p, None)
        .map_err(|e| format!("argon2 params: {e}"))?;
    let hash = PasswordHash::new(&vault.verify_hash).map_err(|e| format!("parse hash: {e}"))?;
    let parameter = |name| hash.params.get_decimal(name).unwrap_or(u32::MAX);
    if !bounded(parameter("m"), parameter("t"), parameter("p")) {
        return Err("vault verification KDF exceeds supported limits".into());
    }
    let params = Params::try_from(&hash).map_err(|e| format!("argon2 hash params: {e}"))?;
    if hash.algorithm.as_str() != "argon2id"
        || hash.version != Some(19)
        || !bounded(params.m_cost(), params.t_cost(), params.p_cost())
    {
        return Err("unsupported vault verification parameters".into());
    }
    Ok(())
}

fn save(path: &Path, vault: &VaultFile) -> Result<(), String> {
    let data = serde_json::to_string_pretty(vault).map_err(|e| format!("serialize vault: {e}"))?;
    write_vault_file(path, data.as_bytes())
}

/// Write the vault file with owner-only permissions on Unix.
fn write_vault_file(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(|e| format!("write vault: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set vault permissions: {e}"))?;
    }
    Ok(())
}

// Keep `Zeroize` and `Zeroizing` referenced so the crate's zeroize feature is used.
#[allow(dead_code)]
fn _zeroize_anchor(k: Zeroizing<[u8; 32]>) {
    let mut k = k;
    k.zeroize();
}

#[cfg(test)]
#[path = "master_password_malformed_tests.rs"]
mod malformed_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_verify_password() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());

        assert!(!is_set());
        set_password("correct horse battery staple").expect("set");
        assert!(is_set());
        assert!(verify_password("correct horse battery staple").expect("verify"));
        assert!(!verify_password("wrong password").expect("verify wrong"));
    }

    #[test]
    fn set_password_rejects_short() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());
        assert!(set_password("short").is_err());
    }

    #[test]
    fn credential_round_trip() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());
        set_password("master-password-123").expect("set");

        store_credential("ssh-pass", "master-password-123", "sesame").expect("store");
        let plain = load_credential("ssh-pass", "master-password-123").expect("load");
        assert_eq!(plain, "sesame");
    }

    #[test]
    fn has_and_remove_credential_without_unlock() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());

        // No vault file at all: has/remove both report absent without erroring.
        assert!(!has_credential("ssh-a"));
        assert!(!remove_credential("ssh-a").expect("remove without vault"));

        set_password("master-password-123").expect("set");
        assert!(!has_credential("ssh-a"));
        store_credential("ssh-a", "master-password-123", "sesame").expect("store");
        assert!(has_credential("ssh-a"));

        // Removal rewrites the vault without decrypting (no master password).
        assert!(remove_credential("ssh-a").expect("remove"));
        assert!(!has_credential("ssh-a"));
        assert!(!remove_credential("ssh-a").expect("remove again"));
        // The rest of the vault is untouched (verify hash still validates).
        assert!(verify_password("master-password-123").expect("verify"));
    }

    #[test]
    fn vault_dir_override_redirects_the_file() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());
        set_password("master-password-123").expect("set default");

        let alt = tempfile::tempdir().expect("alt dir");
        set_vault_dir(Some(alt.path().to_path_buf()));
        assert!(!is_set()); // override points at an empty dir
        set_password("another-master-123").expect("set in override");
        store_credential("ssh-b", "another-master-123", "open").expect("store");
        assert!(alt.path().join("vault.json").exists());

        set_vault_dir(None);
        assert!(is_set()); // back to the default-dir vault
        assert!(verify_password("master-password-123").expect("verify default"));
    }

    #[test]
    fn change_password_reencrypts_credentials() {
        let _guard = crate::test_config_guard();
        let dir = tempfile::tempdir().expect("temp dir");
        crate::set_test_config_dir(dir.path().to_path_buf());
        set_password("correct horse battery staple").expect("set");
        store_credential("ssh-pass", "correct horse battery staple", "sesame").expect("store");

        // Wrong old password is rejected.
        assert!(change_password("wrong old", "new-password-123").is_err());

        change_password("correct horse battery staple", "new-password-123").expect("change");
        assert!(verify_password("new-password-123").expect("verify new"));
        assert!(!verify_password("correct horse battery staple").expect("verify old fails"));

        // Credential survives the re-encryption under the new password.
        let plain = load_credential("ssh-pass", "new-password-123").expect("load after change");
        assert_eq!(plain, "sesame");
    }
}
