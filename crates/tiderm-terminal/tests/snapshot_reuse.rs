//! Reused drawing buffers must retire glyphs hidden by the fold projection.
use tiderm_terminal::{spans, Font, RenderOpts, SoftwareRenderer, TerminalPane};

#[test]
fn folding_retires_glyphs_from_marker_tails_and_empty_projected_rows() {
    let font = Font::from_bytes(
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
        14.0,
    )
    .unwrap();
    let opts = RenderOpts {
        cursor_on: false,
        cursor_style: 0,
        gutter: false,
    };
    let mut pane = TerminalPane::new(24, 8, Box::new(SoftwareRenderer::new()));
    pane.feed("\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n\x1b]133;C\x07ZZZZZZZZZZZZZZZZZZZZ\r\nZZZZ\r\nZZZZ\r\nZZZZ\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let id = pane.command_blocks()[0].id;
    let hidden_glyphs = |records: &[u32]| {
        records
            .as_chunks::<{ spans::REC_STRIDE }>()
            .0
            .iter()
            .filter(|record| record[0] & 0xff == spans::KIND_TEXT && record[4] == u32::from('Z'))
            .count()
    };
    assert_eq!(hidden_glyphs(&pane.spans(&font, &opts)), 32);
    assert!(pane.command_set_folded(id, true));
    let folded = pane.spans(&font, &opts);
    assert_eq!(
        hidden_glyphs(&folded),
        0,
        "the folded marker tail and projected blank rows retained old glyphs"
    );
    let prompt_baseline = 2 * font.cell_h + font.ascent.max(0) as u32;
    assert!(
        folded
            .as_chunks::<{ spans::REC_STRIDE }>()
            .0
            .iter()
            .filter(|record| record[0] & 0xff == spans::KIND_TEXT)
            .all(|record| record[1] >> 16 <= prompt_baseline),
        "blank projected rows retained glyphs below the prompt"
    );
    assert!(pane.command_set_folded(id, false));
    assert_eq!(hidden_glyphs(&pane.spans(&font, &opts)), 32);

    pane.clear();
    assert_eq!(hidden_glyphs(&pane.spans(&font, &opts)), 0);
}
