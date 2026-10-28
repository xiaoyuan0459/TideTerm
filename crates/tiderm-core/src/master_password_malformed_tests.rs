use super::*;

const PASSWORD: &str = "isolated-vault-test-password";

#[test]
fn malformed_nonce_returns_error_and_preserves_vault() {
    let _guard = crate::test_config_guard();
    let dir = tempfile::tempdir().unwrap();
    crate::set_test_config_dir(dir.path().to_path_buf());
    set_password(PASSWORD).unwrap();
    store_credential("fixture", PASSWORD, "synthetic secret").unwrap();
    let path = dir.path().join("vault.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    use base64::Engine;
    for length in [0, 3, 11, 13] {
        let mut damaged = original.clone();
        damaged["credentials"]["fixture"]["iv"] = base64::engine::general_purpose::STANDARD
            .encode(vec![0; length])
            .into();
        let bytes = serde_json::to_vec(&damaged).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(
            load_credential("fixture", PASSWORD).is_err(),
            "IV length {length}"
        );
        assert!(change_password(PASSWORD, "replacement-password").is_err());
        assert_eq!(
            fs::read(&path).unwrap(),
            bytes,
            "failed change rewrote vault"
        );
    }
    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    assert_eq!(
        load_credential("fixture", PASSWORD).unwrap(),
        "synthetic secret"
    );
}

#[test]
fn unsupported_metadata_is_rejected_before_crypto_or_mutation() {
    let _guard = crate::test_config_guard();
    let dir = tempfile::tempdir().unwrap();
    crate::set_test_config_dir(dir.path().to_path_buf());
    set_password(PASSWORD).unwrap();
    store_credential("fixture", PASSWORD, "synthetic secret").unwrap();
    let path = dir.path().join("vault.json");
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let hash = original["verify_hash"].as_str().unwrap();
    let cases = [
        ("/version", serde_json::json!(2)),
        ("/kdf/m", serde_json::json!(u32::MAX)),
        ("/kdf/t", serde_json::json!(u32::MAX)),
        ("/kdf/p", serde_json::json!(u32::MAX)),
        (
            "/verify_hash",
            hash.replace("m=65536", "m=4294967295").into(),
        ),
        ("/verify_hash", hash.replace("t=3", "t=4294967295").into()),
        ("/verify_hash", hash.replace("p=4", "p=4294967295").into()),
    ];
    for (field, value) in cases {
        let mut damaged = original.clone();
        *damaged.pointer_mut(field).unwrap() = value;
        let bytes = serde_json::to_vec(&damaged).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(verify_password(PASSWORD).is_err(), "accepted {field}");
        assert!(
            load_credential("fixture", PASSWORD).is_err(),
            "loaded {field}"
        );
        assert!(store_credential("other", PASSWORD, "value").is_err());
        assert!(change_password(PASSWORD, "replacement-password").is_err());
        assert!(remove_credential("fixture").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes, "rewrote {field}");
    }
}
