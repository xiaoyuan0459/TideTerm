use super::super::store::session_config::{SessionKind, SshAuth};
use super::*;
use serde_json::json;

fn fixture(cfg: &SessionConfig) -> String {
    json!({"version":1,"layout":{"type":"group","active":0,"pages":[
        {"type":"page","active":0,"tree":{"type":"pane","session":0}}
    ]},"sessions":[session_store::to_core(cfg)]})
    .to_string()
}

#[test]
fn recovery_file_round_trip_strips_injected_credentials_and_keeps_connection_options() {
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.host = "fixture.invalid".into();
    ssh.username = "fixture-user".into();
    ssh.auth = SshAuth::Password;
    cfg.meta.options.ssh.proxy.username = "fixture-proxy".into();
    let mut input: serde_json::Value = serde_json::from_str(&fixture(&cfg)).unwrap();
    input["sessions"][0]["ssh"]["auth"]["password"] = "synthetic-ssh-secret".into();
    input["sessions"][0]["meta"]["options"]["ssh"]["proxy"]["password"] =
        "synthetic-proxy-secret".into();
    let dir = tempfile::tempdir().unwrap();
    write_at(dir.path(), "window-one", &input.to_string()).unwrap();
    let persisted = std::fs::read_to_string(dir.path().join("window-one.json")).unwrap();
    assert!(!persisted.contains("synthetic-ssh-secret"));
    assert!(!persisted.contains("synthetic-proxy-secret"));
    let restored = Snapshot::parse(&read_at(dir.path(), "window-one").unwrap()).unwrap();
    let result = session_store::from_core(&restored.sessions[0]);
    assert_eq!(result.ssh.unwrap().host, "fixture.invalid");
    assert_eq!(workspace_secret_flags(&persisted, 0).unwrap(), 5);
    assert!(file_at(dir.path(), "../escape").is_err());
}

#[test]
fn invalid_recovery_points_cannot_replace_previous_file() {
    let cfg = SessionConfig::blank(SessionKind::Local);
    let original = fixture(&cfg);
    let dir = tempfile::tempdir().unwrap();
    write_at(dir.path(), "window", &original).unwrap();
    let before = std::fs::read(dir.path().join("window.json")).unwrap();
    let value: serde_json::Value = serde_json::from_str(&original).unwrap();
    let mut unknown = value.clone();
    unknown["version"] = 2.into();
    let mut out_of_range = value.clone();
    out_of_range["layout"]["pages"][0]["tree"]["session"] = 1.into();
    let mut duplicate = value.clone();
    duplicate["layout"]["pages"] =
        json!([value["layout"]["pages"][0], value["layout"]["pages"][0]]);
    let mut wrong_domain = value.clone();
    wrong_domain["layout"] = json!({"type":"pane","session":0});
    for invalid in [
        "{".into(),
        unknown.to_string(),
        out_of_range.to_string(),
        duplicate.to_string(),
        wrong_domain.to_string(),
        " ".repeat(MAX_BYTES + 1),
    ] {
        assert!(write_at(dir.path(), "window", &invalid).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("window.json")).unwrap(),
            before
        );
    }
    std::fs::write(dir.path().join("window.json"), b"corrupt").unwrap();
    assert!(read_at(dir.path(), "window").is_err());
    assert_eq!(
        std::fs::read(dir.path().join("window.json")).unwrap(),
        b"corrupt"
    );
}

#[test]
fn window_files_are_independent_and_io_errors_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = SessionConfig::blank(SessionKind::Local);
    write_at(dir.path(), "first", &fixture(&cfg)).unwrap();
    let mut second = cfg.clone();
    second.name = "second window".into();
    write_at(dir.path(), "second", &fixture(&second)).unwrap();
    assert!(!read_at(dir.path(), "first")
        .unwrap()
        .contains("second window"));
    assert!(read_at(dir.path(), "second")
        .unwrap()
        .contains("second window"));
    let occupied = dir.path().join("occupied");
    std::fs::write(&occupied, b"keep").unwrap();
    assert!(write_at(&occupied, "first", &fixture(&cfg)).is_err());
    assert_eq!(std::fs::read(occupied).unwrap(), b"keep");
}
