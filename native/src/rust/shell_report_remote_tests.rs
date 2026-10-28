//! The authenticated SSH channel owns byte admission and absence of exec/SFTP.
use super::*;
use crate::rust::{
    command_completion::{completion_context, completion_query, completion_shell_draft},
    store::{SessionConfig, SessionKind},
    term,
};
use crate::session_config::SshAuth;

#[test]
#[ignore = "requires isolated ssh_sftp_peer.py loopback fixture and TEST_SSH_* configuration"]
fn ssh_report_preview_is_silent_and_confirmation_uses_only_the_terminal_channel_once() {
    let _network = crate::rust::term_net_tests::net_serial();
    let _input = crate::rust::term_tests::input_lock_guard();
    crate::rust::term_ssh_tests::isolate_known_hosts();
    let mut cfg = SessionConfig::blank(SessionKind::Ssh);
    cfg.meta.sftp_enabled = false;
    let ssh = cfg.ssh.as_mut().unwrap();
    ssh.host = "127.0.0.1".into();
    ssh.port = std::env::var("TEST_SSH_PORT")
        .expect("fixture port")
        .parse()
        .unwrap();
    ssh.username = "fixture".into();
    ssh.password = "fixture-password".into();
    ssh.auth = SshAuth::Password;
    let tab = term::open_tab_with_config(&cfg);
    assert!(crate::rust::term_net_tests::wait_until(
        Duration::from_secs(8),
        || {
            term::term_pump();
            let c = completion_context(tab);
            c.ready && c.cwd == "/env-a"
        }
    ));
    let stats_file = std::path::PathBuf::from(std::env::var("TEST_SSH_KNOWN_HOSTS").unwrap())
        .parent()
        .unwrap()
        .join("stats.json");
    let stats = || -> serde_json::Value {
        // The fixture replaces this small counter file between operations.
        for _ in 0..30 {
            if let Ok(data) = std::fs::read(&stats_file)
                && let Ok(value) = serde_json::from_slice(&data)
            {
                return value;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("fixture counters unavailable");
    };
    for mode in 0..5 {
        let before = stats();
        let preview = if mode == 4 {
            shell_kube_prepare(
                tab,
                1,
                &KubeScope {
                    resource: 0,
                    config: "/fixture/config".into(),
                    context: "fixture".into(),
                    namespace_name: "fixture".into(),
                },
            )
        } else if mode == 3 {
            shell_docker_prepare(tab, 1, "fixture")
        } else if mode == 2 {
            shell_git_prepare(tab, 1)
        } else if mode == 1 {
            shell_resolution_prepare(tab, 1, "TIDE_REMOTE")
        } else {
            shell_report_prepare(tab, 1)
        };
        assert_eq!(preview.state, 1);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(stats()["terminal_bytes"], before["terminal_bytes"]);
        if mode == 4 {
            assert_eq!(shell_report_send(tab, &preview.token), 15);
            assert_eq!(shell_kube_send(tab, &preview.token, false), 15);
            assert_eq!(stats()["terminal_bytes"], before["terminal_bytes"]);
            assert_eq!(shell_kube_send(tab, &preview.token, true), 2);
        } else {
            assert_eq!(shell_report_send(tab, &preview.token), 2);
        }
        assert_eq!(shell_report_send(tab, &preview.token), 6);
        assert!(crate::rust::term_net_tests::wait_until(
            Duration::from_secs(8),
            || {
                term::term_pump();
                shell_report_poll(tab).state == 3
            }
        ));
        let report = shell_report_poll(tab);
        assert_eq!(report.platform, 1);
        if mode == 4 {
            assert_eq!(report.kube.namespace_name, "fixture");
            let batch = completion_query(tab, "kubectl describe pod remote");
            let item = batch.items.iter().find(|i| i.kind == 11).unwrap();
            assert_eq!(item.reference, "remote-uid");
            assert_eq!(item.command,"MSYS2_ARG_CONV_EXCL='*' kubectl '--kubeconfig=/fixture/config' '--context=fixture' '--namespace=fixture' describe pod 'remote-pod'");
            assert_eq!(
                crate::rust::command_completion::completion_kube_draft(
                    &batch.context,
                    &batch.shell.token,
                    "kubectl describe pod remote",
                    &item.reference
                ),
                item.command
            );
        } else if mode == 3 {
            assert_eq!(report.docker_context, "fixture");
            let batch = completion_query(tab, "docker inspect REMOTE_");
            let item = batch.items.iter().find(|i| i.kind == 10).unwrap();
            assert_eq!(item.label, "REMOTE_DOCKER_ONLY");
            assert_eq!(
                item.command,
                format!(
                    "docker --context 'fixture' container inspect '{}'",
                    "f".repeat(64)
                )
            );
            assert_eq!(
                crate::rust::command_completion::completion_docker_draft(
                    &batch.context,
                    &batch.shell.token,
                    "docker inspect REMOTE_",
                    &item.reference
                ),
                item.command
            );
            for query in ["docker logs R", "docker logs REMOTE_", "docker logs OTHER"] {
                completion_query(tab, query);
            }
        } else if mode == 2 {
            assert!(report.git);
            assert_eq!(report.repository, "/REMOTE_ONLY/repo.git");
            let batch = completion_query(tab, "git show remote");
            let item = batch.items.iter().find(|i| i.kind == 9).unwrap();
            assert_eq!(item.command, "git show 'refs/tags/remote-v1'");
            assert_eq!(
                crate::rust::command_completion::completion_git_draft(
                    &batch.context,
                    &batch.shell.token,
                    "git show remote",
                    &item.reference
                ),
                item.command
            );
            assert!(completion_query(tab, "git switch remote")
                .items
                .iter()
                .any(|i| i.kind == 8));
        } else if mode == 1 {
            assert_eq!(report.query, "TIDE_REMOTE");
            assert_eq!(report.kind, 5);
            assert_eq!(report.location, "/REMOTE_ONLY/TIDE_REMOTE");
            for query in [
                "T",
                "TIDE_",
                "TIDE_REM",
                "TIDE_REMOTE",
                "OTHER",
                "echo TIDE_REMOTE",
            ] {
                let suggestions = completion_query(tab, query);
                let matching = "TIDE_REMOTE".starts_with(query);
                assert_eq!(suggestions.items.iter().any(|i| i.kind == 7), matching);
                if matching {
                    assert_eq!(suggestions.shell.location, "/REMOTE_ONLY/TIDE_REMOTE");
                }
            }
            let suggestions = completion_query(tab, "TIDE_");
            assert_eq!(
                completion_shell_draft(&suggestions.context, &suggestions.shell.token),
                "TIDE_REMOTE"
            );
            assert!(
                completion_shell_draft(&suggestions.context, &suggestions.shell.token).is_empty()
            );
            assert!(!completion_query(tab, "TIDE_")
                .items
                .iter()
                .any(|i| i.kind == 7));
        } else {
            assert_eq!(report.path, "/REMOTE_REPORT_ONLY:/remote space::relative");
        }
        let after = stats();
        assert_eq!(
            after["terminal_bytes"].as_u64().unwrap() - before["terminal_bytes"].as_u64().unwrap(),
            preview.command.len() as u64 + 1
        );
        for key in ["exec", "sftp_open", "file_open", "connections"] {
            assert_eq!(after[key], before[key]);
        }
    }
    term::term_close_tab(tab);
}
