#[test]
fn favorite_defaults_to_false_for_legacy_json() {
    // G18: 旧版 sessions.json 的 meta 无 favorite 字段 → 反序列化为 false。
    #[derive(serde::Deserialize)]
    struct Probe {
        #[serde(default)]
        favorite: bool,
    }
    let probe: Probe = serde_json::from_str("{}").unwrap();
    assert!(!probe.favorite);
}

use super::*;

#[test]
fn duplicate_session_ids_keep_first_and_back_up_original() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sessions.json");
    let bytes =
        serde_json::to_vec(&vec![sample("same", "first"), sample("same", "second")]).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let store = SessionStore::load_at(path.clone()).unwrap();
    assert_eq!(store.len(), 1);
    assert_eq!(store.find("same").unwrap().name, "first");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        std::fs::read(path.with_extension("json.duplicates.bak")).unwrap(),
        bytes
    );
    store.save().unwrap();
    assert_eq!(SessionStore::load_at(path).unwrap().len(), 1);
}

fn sample(id: &str, name: &str) -> SessionConfig {
    SessionConfig {
        id: id.into(),
        name: name.into(),
        kind: "ssh".into(),
        group: None,
        tags: Vec::new(),
        sort_order: 0,
        local: None,
        ssh: Some(SshConnectionConfig {
            host: "example.com".into(),
            port: 22,
            username: "admin".into(),
            auth: SshAuth::Password {
                password: "secret".into(),
            },
            connect_timeout_secs: 30,
            reconnect_enabled: false,
            max_reconnect_attempts: 3,
            reconnect_interval_secs: 2,
            remember_credentials: false,
        }),
        telnet: None,
        serial: None,
        tcp: None,
        meta: None,
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-01".into(),
    }
}

fn temp_store() -> (SessionStore, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SessionStore {
        sessions: Vec::new(),
        path: dir.path().join("sessions.json"),
    };
    (store, dir)
}

#[test]
fn store_new_exposes_backing_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.json");
    let store = SessionStore::new(path.clone());
    assert!(store.is_empty());
    assert_eq!(store.path(), path.as_path());
    assert!(!path.exists());
}

#[test]
fn store_upsert_inserts_new() {
    let (mut store, _dir) = temp_store();
    store.upsert(sample("a", "A"));
    assert_eq!(store.len(), 1);
    assert!(store.find("a").is_some());
}

#[test]
fn store_upsert_updates_existing() {
    let (mut store, _dir) = temp_store();
    store.upsert(sample("a", "A"));
    let mut updated = sample("a", "A2");
    updated.name = "renamed".into();
    store.upsert(updated);
    assert_eq!(store.len(), 1);
    assert_eq!(store.find("a").unwrap().name, "renamed");
}

#[test]
fn store_remove_returns_removed_flag() {
    let (mut store, _dir) = temp_store();
    store.upsert(sample("a", "A"));
    assert!(store.remove("a"));
    assert!(!store.remove("a"));
    assert!(store.is_empty());
}

#[test]
fn store_move_before_reorders_and_renumbers() {
    let (mut store, _dir) = temp_store();
    store.upsert(sample("a", "A"));
    store.upsert(sample("b", "B"));
    store.upsert(sample("c", "C"));
    assert!(store.move_before("c", "a"));
    assert_eq!(
        store
            .sessions()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "a", "b"]
    );
    assert_eq!(store.sessions()[0].sort_order, 0);
    assert_eq!(store.sessions()[2].sort_order, 2);
    assert!(!store.move_before("missing", "a"));
}

#[test]
fn store_save_and_load_round_trips() {
    let (mut store, dir) = temp_store();
    store.upsert(sample("a", "A"));
    store.save().expect("save");
    let loaded = SessionStore::load_at(dir.path().join("sessions.json")).expect("load");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.find("a").unwrap().name, "A");
}

#[test]
fn ssh_password_not_persisted() {
    let cfg = sample("a", "A");
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(!json.contains("secret"));
}

#[test]
fn remember_credentials_defaults_false_for_legacy_json() {
    // 旧版 sessions.json 的 ssh 段无 remember_credentials 字段 → false；
    // 显式 true（用户勾选记住密码）往返保留。
    let legacy: SessionConfig =
        serde_json::from_str(r#"{"id":"a","name":"A","kind":"ssh","group":null,"local":null,"created_at":"","updated_at":"","ssh":{"host":"h","port":22,"username":"u","auth":{"type":"password"}}}"#)
            .expect("legacy parse");
    assert!(!legacy.ssh.unwrap().remember_credentials);

    let mut flagged = sample("a", "A");
    flagged.ssh.as_mut().unwrap().remember_credentials = true;
    let json = serde_json::to_string(&flagged).expect("serialize");
    let back: SessionConfig = serde_json::from_str(&json).expect("round trip");
    assert!(back.ssh.unwrap().remember_credentials);
}
