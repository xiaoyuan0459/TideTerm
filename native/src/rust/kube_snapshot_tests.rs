use super::*;
fn scope() -> KubeScope {
    KubeScope {
        resource: 0,
        config: "/fixture/space's.json".into(),
        context: "fixture".into(),
        namespace_name: "fixture".into(),
    }
}
fn body(s: &KubeScope, rows: &str) -> Vec<u8> {
    format!("5;receipt;b;0;{};{}", scope_hex(s), hex(rows.as_bytes())).into_bytes()
}
#[test]
fn scoped_pod_identity_budget_and_draft_literals_are_strict() {
    let s = scope();
    assert!(valid_scope(&s, 1));
    for config in [
        "relative",
        "//server/config",
        "C:\\bad\nname",
        "/bad\"name",
        "/bad‘name",
    ] {
        let mut invalid = s.clone();
        invalid.config = config.into();
        assert!(!valid_scope(&invalid, 1));
    }
    for context in ["", "fixture;echo", "$(command)"] {
        let mut invalid = s.clone();
        invalid.context = context.into();
        assert!(!valid_scope(&invalid, 2));
    }
    let rows = "\"uid-1\"|\"pod-a\"|\"fixture\"";
    let mut r = decode(&body(&s, rows), "receipt", 1, &s).unwrap();
    r.state = 3;
    assert_eq!(suggestions(&r,"kubectl get pod pod")[0].command,"MSYS2_ARG_CONV_EXCL='*' kubectl '--kubeconfig=/fixture/space'\\''s.json' '--context=fixture' '--namespace=fixture' get pod 'pod-a'");
    let mut other = s.clone();
    other.namespace_name = "other".into();
    assert!(decode(&body(&s, rows), "receipt", 1, &other).is_none());
    for rows in [
        "\"uid-1\"|\"pod-a\"|\"other\"",
        "\"uid-1\"|\"bad\\u0000name\"|\"fixture\"",
        "\"uid-1\"|\"pod-a\"|\"fixture\"\n\"uid-1\"|\"pod-b\"|\"fixture\"",
        "\"uid-1\"|\"-bad\"|\"fixture\"",
    ] {
        assert!(decode(&body(&s, rows), "receipt", 1, &s).is_none());
    }
    let rows = (0..33)
        .map(|i| format!("\"uid-{i}\"|\"pod-{i}\"|\"fixture\""))
        .collect::<Vec<_>>()
        .join("\n");
    let r = decode(&body(&s, &rows), "receipt", 1, &s).unwrap();
    assert!(r.truncated);
    assert_eq!(r.resources.len(), 32);
    assert!(decode(&body(&s, &"x".repeat(6145)), "receipt", 1, &s).is_none());
    for q in [
        "kubectl delete pod ",
        "kubectl get pod a;run",
        "kubectl get pod 'a",
        "kubectl get pod $(x)",
    ] {
        assert!(suggestions(&r, q).is_empty());
    }
    for (resource, aliases) in [
        (0, vec!["pod", "pods", "po"]),
        (1, vec!["service", "services", "svc"]),
        (2, vec!["deployment", "deployments", "deploy"]),
    ] {
        let mut selected = s.clone();
        selected.resource = resource;
        let mut report = decode(
            &body(&selected, "\"uid-one\"|\"entry-a\"|\"fixture\""),
            "receipt",
            1,
            &selected,
        )
        .unwrap();
        report.state = 3;
        for alias in aliases {
            for verb in ["get", "describe"] {
                let query = format!("kubectl {verb} {alias} entry");
                let draft = suggestions(&report, &query);
                assert_eq!(draft.len(), 1);
                assert!(draft[0].command.ends_with(&format!(
                    "{verb} {} 'entry-a'",
                    resource_name(resource).unwrap()
                )));
                assert_eq!(provider(&query), Some(4 + resource));
            }
        }
        let mut wrong = selected.clone();
        wrong.resource = (resource + 1) % 3;
        assert!(
            decode(&body(&selected, ""), "receipt", 1, &wrong).is_none(),
            "resource type must bind the receipt"
        );
        assert!(suggestions(
            &report,
            &format!(
                "kubectl get {} entry",
                resource_name(wrong.resource).unwrap()
            )
        )
        .is_empty());
    }
}
#[test]
#[ignore = "requires isolated kubectl CLI, TLS API and synthetic exec plugin"]
fn real_kubectl_scope_plugin_and_metadata_contract() {
    use std::{path::PathBuf, process::Command};
    let root = PathBuf::from(std::env::var("TIDETERM_TEST_KUBE_ROOT").expect("fixture root"));
    assert!(root
        .canonicalize()
        .unwrap()
        .starts_with(std::env::temp_dir().canonicalize().unwrap()));
    let exe = PathBuf::from(std::env::var("TIDETERM_TEST_KUBE_EXE").unwrap());
    for resource in 0..3 {
        for mode in 0..6 {
            std::fs::write(
                root.join("state.json"),
                match mode {
                    0 => r#"{"count":2}"#,
                    1 => r#"{"count":40}"#,
                    2 => r#"{"fail":true}"#,
                    3 => r#"{"nul":true}"#,
                    4 => r#"{"wrong_namespace":true}"#,
                    _ => r#"{"count":40,"long_names":true}"#,
                },
            )
            .unwrap();
            for dialect in [1, 2] {
                let raw = root
                    .join("scope space's.json")
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                let config = raw;
                let s = KubeScope {
                    resource,
                    config,
                    context: if dialect == 1 {
                        "/fixture/context"
                    } else {
                        "fixture"
                    }
                    .into(),
                    namespace_name: "fixture".into(),
                };
                let before: usize = std::fs::read_to_string(root.join("plugin-count.txt"))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut p = Command::new(if dialect == 1 {
                    "C:/Program Files/Git/bin/bash.exe"
                } else {
                    "pwsh"
                });
                p.env("HOME", &root)
                    .env("USERPROFILE", &root)
                    .env("KUBECONFIG", root.join("wrong-config"));
                p.env(
                    "PATH",
                    std::env::join_paths(
                        std::iter::once(exe.parent().unwrap().to_path_buf())
                            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
                    )
                    .unwrap(),
                );
                if dialect == 1 {
                    p.args(["--noprofile", "--norc", "-c"]);
                } else {
                    p.args(["-NoProfile", "-NonInteractive", "-Command"]);
                }
                let mut local_probe = Command::new(p.get_program());
                local_probe.args(p.get_args());
                for (key, value) in p.get_envs() {
                    if let Some(value) = value {
                        local_probe.env(key, value);
                    }
                }
                let out = p
                    .arg(command("receipt", dialect, &s).unwrap())
                    .output()
                    .unwrap();
                assert!(out.status.success());
                let mut wire = crate::rust::shell_report_wire::Wire::default();
                let (visible, frames) = wire.feed(&out.stdout);
                assert!(visible.is_empty());
                assert_eq!(frames.len(), 1);
                let decoded = decode(&frames[0], "receipt", dialect, &s);
                if mode < 2 {
                    let r = decoded.expect("metadata rejected");
                    assert_eq!(
                        r.kube_error,
                        0,
                        "CLI failed: {}",
                        String::from_utf8_lossy(&out.stderr)
                    );
                    assert_eq!(r.resources.len(), if mode == 0 { 2 } else { 32 });
                    assert_eq!(r.truncated, mode == 1);
                    if mode == 0 {
                        // Check accepted scope locally, without running the Pod
                        // draft or invoking authentication/API requests.
                        let mut ready = r;
                        ready.state = 3;
                        let kind = resource_name(resource).unwrap();
                        let draft = suggestions(&ready, &format!("kubectl get {kind} {kind}"))[0]
                            .command
                            .clone();
                        let probe = draft
                            .strip_suffix(&format!("get {kind} '{kind}-000'"))
                            .unwrap()
                            .to_owned()
                            + "config view --minify -o 'jsonpath={.contexts[0].name}'";
                        let result = local_probe.arg(probe).output().unwrap();
                        assert!(
                            result.status.success(),
                            "scope probe: {}",
                            String::from_utf8_lossy(&result.stderr)
                        );
                        assert_eq!(String::from_utf8(result.stdout).unwrap(), s.context);
                    }
                } else {
                    assert!(decoded.is_none_or(|r| r.kube_error != 0 && r.resources.is_empty()));
                }
                let after: usize = std::fs::read_to_string(root.join("plugin-count.txt"))
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(after, before + 1);
                let stats: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(root.join("stats.json")).unwrap())
                        .unwrap();
                assert_eq!(stats["forbidden"], 0);
                assert_eq!(stats["bad_scope"], 0);
                assert_eq!(stats["bad_auth"], 0);
            }
        }
    }
    std::fs::write(root.join("state.json"), r#"{"count":2}"#).unwrap();
}
