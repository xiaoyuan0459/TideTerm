//! Credential admission and scope/member lifetime, using the private receipt adapter.
use super::*;
#[test]
fn credential_gate_precedes_input_and_replaced_scope_invalidates_uid() {
    use crate::rust::{command_completion::*, term};
    let _n = crate::rust::term_net_tests::net_serial();
    let _i = crate::rust::term_tests::input_lock_guard();
    let tab = term::term_new_tab();
    assert!(crate::rust::term_net_tests::wait_until(
        std::time::Duration::from_secs(8),
        || {
            term::term_pump();
            completion_context(tab).ready
        }
    ));
    let s = crate::bridge::completion::completion_ffi::KubeScope {
        resource: 0,
        config: "/fixture/space's.json".into(),
        context: "fixture".into(),
        namespace_name: "fixture".into(),
    };
    let preview = shell_kube_prepare(tab, 1, &s);
    assert_eq!(preview.state, 1);
    assert_eq!(shell_report_send(tab, &preview.token), 15);
    assert_eq!(shell_kube_send(tab, &preview.token, false), 15);
    assert_eq!(shell_report_poll(tab).state, 1);
    // Only the receipt adapter is simulated here. SSH owns actual byte admission.
    term::with_tab(tab, |st| {
        st.context.shell.pending.as_mut().unwrap().sent = true;
        let row = b"\"uid-1\"|\"pod-a\"|\"fixture\"";
        let hex: String = row.iter().map(|b| format!("{b:02x}")).collect();
        assert!(st
            .context
            .shell
            .output(
                format!(
                    "\x1b]777;tideterm-env;5;{};b;0;{};{hex}\x07",
                    preview.token,
                    super::super::kube_snapshot::scope_hex(&s)
                )
                .as_bytes()
            )
            .is_empty());
    });
    let batch = completion_query(tab, "kubectl get pod pod");
    assert_eq!(batch.items.iter().filter(|i| i.kind == 11).count(), 1);
    assert!(!completion_kube_draft(
        &batch.context,
        &preview.token,
        "kubectl get pod pod",
        "uid-1"
    )
    .is_empty());
    assert!(completion_kube_draft(
        &batch.context,
        &preview.token,
        "kubectl get pod pod",
        "other"
    )
    .is_empty());
    let mut other = s.clone();
    other.namespace_name = "other".into();
    shell_kube_prepare(tab, 1, &other);
    assert!(completion_kube_draft(
        &batch.context,
        &preview.token,
        "kubectl get pod pod",
        "uid-1"
    )
    .is_empty());
    term::term_close_tab(tab);
}
