use super::*;
const FIRST: &str = "fixture-lock-password-1";
const SECOND: &str = "fixture-lock-password-2";

#[test]
fn persisted_password_and_timeout_require_old_password_to_change_or_remove() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screen_lock.json");
    let store = PasswordStore::new(path.clone());
    assert!(!store.is_set());
    assert!(store.save("", "short", false, 15).is_err());
    store.save("", FIRST, false, 15).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains(FIRST));
    // A fresh owner must recover both settings from disk, without cached secrets.
    let reopened = PasswordStore::new(path.clone());
    reopened.verify(FIRST).unwrap();
    assert!(reopened.verify("").is_err());
    assert!(reopened.save("wrong", SECOND, false, 5).is_err());
    assert!(reopened.save("wrong", "", true, 0).is_err());
    assert!(reopened.save("wrong", "", false, 0).is_err());
    reopened.verify(FIRST).unwrap();
    reopened.save(FIRST, "", false, 5).unwrap();
    let timeout_only = PasswordStore::new(path.clone());
    assert_eq!(timeout_only.idle.lock().unwrap().minutes, 5);
    timeout_only.verify(FIRST).unwrap();
    reopened.save(FIRST, SECOND, false, 15).unwrap();
    assert!(reopened.verify(FIRST).is_err());
    PasswordStore::new(path.clone()).verify(SECOND).unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains(SECOND));
    reopened.save(SECOND, "", true, 0).unwrap();
    let removed = PasswordStore::new(path);
    assert!(!removed.is_set());
    assert_eq!(removed.idle.lock().unwrap().minutes, 0);
}

#[test]
fn repeated_failures_are_throttled_and_corrupt_config_never_becomes_passwordless() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screen_lock.json");
    let store = PasswordStore::new(path.clone());
    store.save("", FIRST, false, 15).unwrap();
    for _ in 0..5 {
        assert!(store.verify("wrong").is_err());
    }
    assert!(store.verify(FIRST).unwrap_err().contains("秒后重试"));
    assert!(store.save(FIRST, "", true, 0).is_err());
    fs::write(&path, "{broken").unwrap();
    let reopened = PasswordStore::new(path.clone());
    assert!(reopened.is_set());
    assert!(reopened.verify("").is_err());
    assert!(reopened.save("", "", true, 0).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
}

#[test]
fn v1_password_upgrades_without_replacement_and_invalid_timeout_preserves_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("screen_lock.json");
    let store = PasswordStore::new(path.clone());
    store.save("", FIRST, false, 5).unwrap();
    let mut legacy: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    legacy["version"] = 1.into();
    legacy.as_object_mut().unwrap().remove("idle_minutes");
    fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let reopened = PasswordStore::new(path.clone());
    assert_eq!(reopened.idle.lock().unwrap().minutes, DEFAULT_IDLE_MINUTES);
    reopened.verify(FIRST).unwrap();
    reopened.save(FIRST, "", false, 30).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(reopened.save(FIRST, "", true, 1441).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    let upgraded = PasswordStore::new(path);
    upgraded.verify(FIRST).unwrap();
    assert_eq!(upgraded.idle.lock().unwrap().minutes, 30);
}

#[test]
fn inactivity_boundary_resets_only_on_activity_and_zero_disables_it() {
    let start = Instant::now();
    let mut idle = IdleTimer {
        minutes: 1,
        last_activity: start,
    };
    assert!(!idle.due(start + Duration::from_secs(59)));
    assert!(idle.due(start + Duration::from_secs(60)));
    idle.activity(start + Duration::from_secs(30));
    assert!(!idle.due(start + Duration::from_secs(89)));
    assert!(idle.due(start + Duration::from_secs(90)));
    idle.minutes = 0;
    assert!(!idle.due(start + Duration::from_secs(86400)));
    idle.minutes = 15;
    assert!(!idle.due(start + Duration::from_secs(929)));
    assert!(idle.due(start + Duration::from_secs(930)));
}
