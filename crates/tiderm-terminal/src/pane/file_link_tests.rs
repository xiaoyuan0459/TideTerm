use super::TerminalPane;
use crate::SoftwareRenderer;

#[test]
fn changing_directory_revalidates_the_visible_link_against_real_files() {
    let root = std::env::temp_dir().join(format!("tideterm-link-cwd-{}", std::process::id()));
    std::fs::create_dir_all(root.join("a")).unwrap();
    std::fs::create_dir_all(root.join("b")).unwrap();
    std::fs::write(root.join("a/note.txt"), b"fixture").unwrap();
    let mut pane = TerminalPane::new(40, 2, Box::new(SoftwareRenderer::new()));
    pane.feed("note.txt");
    pane.set_cwd(Some(root.join("a").to_string_lossy().into_owned()));
    assert!(pane.link_at(2.5, 0.5, 40.0, 2.0).is_some());
    pane.set_cwd(Some(root.join("b").to_string_lossy().into_owned()));
    assert!(pane.link_at(2.5, 0.5, 40.0, 2.0).is_none());
    std::fs::remove_file(root.join("a/note.txt")).unwrap();
    pane.set_cwd(Some(root.join("a").to_string_lossy().into_owned()));
    assert!(pane.link_at(2.5, 0.5, 40.0, 2.0).is_none());
    std::fs::remove_dir_all(root).unwrap();
}
