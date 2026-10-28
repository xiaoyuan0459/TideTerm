use super::*;

fn sample(id: &str, name: &str) -> SessionConfig {
    let mut c = SessionConfig::blank(SessionKind::Local);
    c.id = id.to_string();
    c.meta.label = name.to_string();
    c
}

#[test]
fn loaded_config_keeps_stable_id_on_save() {
    let c = sample("keep-me", "prod");
    let core = to_core(&c);
    assert_eq!(core.id, "keep-me");
}

#[test]
fn new_config_gets_generated_id() {
    let c = SessionConfig::blank(SessionKind::Local);
    assert!(!to_core(&c).id.is_empty());
}

#[test]
fn move_before_reorders_persisted_sessions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = SessionStore::new(dir.path().join("sessions.json"));
    for id in ["a", "b", "c"] {
        store.upsert(to_core(&sample(id, id)));
    }
    store.save().expect("save");
    assert!(move_before_in(&mut store, "c", "a").expect("move"));
    assert_eq!(
        store
            .sessions()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "a", "b"]
    );
}

#[test]
fn rename_and_delete_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = SessionStore::new(dir.path().join("sessions.json"));
    store.upsert(to_core(&sample("s-1", "old")));
    store.save().expect("save");

    rename_in(&mut store, "s-1", "new").expect("rename");
    assert_eq!(store.find("s-1").unwrap().name, "new");

    assert!(delete_in(&mut store, "s-1").expect("delete"));
    assert!(!delete_in(&mut store, "s-1").expect("delete again"));
}

#[test]
fn favorite_round_trips_through_core_mapping() {
    // G18: 收藏标记经 to_core/from_core 往返保持（serde default 兼容旧 sessions.json）。
    let mut cfg =
        crate::session_config::SessionConfig::blank(crate::session_config::SessionKind::Ssh);
    cfg.name = "fav-host".into();
    cfg.meta.favorite = true;
    let core = to_core(&cfg);
    assert!(core.meta.as_ref().unwrap().favorite);
    let back = from_core(&core);
    assert!(back.meta.favorite);

    let mut plain =
        crate::session_config::SessionConfig::blank(crate::session_config::SessionKind::Local);
    plain.name = "plain".into();
    let core2 = to_core(&plain);
    assert!(!core2.meta.as_ref().unwrap().favorite, "默认不收藏");
}

#[test]
fn onekey_binding_survives_save_and_reload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("sessions.json");
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    cfg.id = "onekey-session".into();
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.auth = SshAuth::OneKey;
    ssh.account_id = "account-1".into();
    let mut store = SessionStore::new(path.clone());
    store.upsert(to_core(&cfg));
    store.save().expect("save");
    let loaded = SessionStore::load_at(path).expect("reload");
    let restored = from_core(loaded.find(&cfg.id).expect("saved session"));
    let ssh = restored.ssh.expect("SSH config");
    assert_eq!(ssh.auth, SshAuth::OneKey);
    assert_eq!(ssh.account_id, "account-1");
}
