use super::*;
use crate::rust::{store, term_diag, term_net_tests, term_tests};

#[test]
fn hard_wrapped_empty_powershell_prompt_restores_draft_without_reopening_program_input() {
    let _serial = term_net_tests::net_serial();
    let _input = term_tests::input_lock_guard();
    let (id, mut stream) = peer();
    term::with_tab(id, |st| {
        st.config = store::SessionConfig::blank(store::SessionKind::Local);
        st.config.local.as_mut().unwrap().shell = "powershell".into();
    });
    let metrics = term_diag::term_stats(id);
    term::term_resize_px(
        id,
        16.0 * metrics.cell_w as f32,
        8.0 * metrics.cell_h as f32,
    );
    let mut feed = |bytes: &[u8]| {
        let before = term::with_tab(id, |st| st.preview_revision);
        stream.write_all(bytes).unwrap();
        assert!(term_net_tests::wait_until(Duration::from_secs(5), || {
            term::term_poll(id);
            term::with_tab(id, |st| st.preview_revision) != before
        }));
    };
    feed(b"\x1b[2J\x1b[Hbanner\r\n\x1b]133;A\x07PS D:\\work> \x1b]133;B\x07");
    assert!(
        completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    // ConPTY can repaint with hard CRLFs, leaving the prompt's separator alone
    // on the next row while the old OSC B coordinate still maps into the grid.
    let repaint = b"\x1b[Hbanner\r\nline\r\n\r\n\r\nPS D:\\work\r\n\\fixture>\r\n \x1b[K";
    term::term_key(id, 0, 4, "a");
    feed(repaint);
    assert!(
        !completion::terminal_completion_query(id)
            .batch
            .context
            .ready,
        "a reconstructed prompt must not acknowledge an unconfirmed editor action"
    );
    feed(b"\x1b]133;B\x07");
    assert!(
        completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    feed(b"\x1b[2J\x1b[Hbanner\r\n\x1b]133;A\x07PS D:\\work> \x1b]133;B\x07");
    feed(repaint);
    let empty = completion::terminal_completion_query(id);
    assert!(
        empty.batch.context.ready,
        "an empty repainted prompt is editable"
    );
    assert_eq!(empty.query, "");
    feed(b"echo  ");
    let draft = completion::terminal_completion_query(id);
    assert!(draft.batch.context.ready);
    assert_eq!(
        draft.query, "echo  ",
        "retain user spaces after reanchoring"
    );
    feed(b"\x1b]133;C\x07");
    feed(repaint);
    assert!(
        !completion::terminal_completion_query(id)
            .batch
            .context
            .ready
    );
    term::term_close_tab(id);
}
