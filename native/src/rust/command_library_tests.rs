use super::*;

fn entry(id: u32) -> UserCommand {
    UserCommand {
        id,
        group: "工具".into(),
        name: "状态".into(),
        command: "git status --short".into(),
        is_template: false,
    }
}

#[test]
fn explicit_catalog_round_trip_conflict_and_failed_save_preserve_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let mut library = Library::open(path.clone());
    assert_eq!(library.save(1, &[entry(7)]), 0);
    assert!(load(&path).unwrap() == vec![entry(7)]);
    let original = std::fs::read(&path).unwrap();
    assert_eq!(library.save(1, &[]), 3);
    let mut invalid = entry(8);
    for text in [
        "",
        "echo a\necho b",
        "echo\tsecret",
        "\x1b[31m",
        &"x".repeat(4097),
    ] {
        invalid.command = text.into();
        assert_eq!(library.save(2, &[invalid.clone()]), 2);
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
    assert_eq!(library.save(2, &[entry(7), entry(7)]), 2);
    assert_eq!(
        library.save(2, &(1..=201).map(entry).collect::<Vec<_>>()),
        2
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    // The actual atomic replace fails against a directory; the cached catalog is untouched.
    library.path = dir.path().to_owned();
    assert_eq!(library.save(2, &[]), 4);
    assert!(library.entries == vec![entry(7)]);
    library.path = path.clone();
    assert_eq!(library.save(2, &[]), 0);
    assert!(Library::open(path).entries.is_empty());
}

#[test]
fn broken_oversized_and_future_profiles_are_not_silently_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    for bytes in [
        b"broken".to_vec(),
        br#"{"version":99,"entries":[]}"#.to_vec(),
        vec![b' '; MAX_BYTES as usize + 1],
    ] {
        std::fs::write(&path, &bytes).unwrap();
        let mut library = Library::open(path.clone());
        assert_eq!(library.error, 1);
        assert_eq!(library.save(1, &[entry(1)]), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn old_catalog_stays_literal_until_explicit_template_save_and_version_two_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("commands.json");
    let old = br#"{"version":1,"entries":[{"id":7,"group":"","name":"literal","command":"echo {{name}} $HOME"}]}"#;
    std::fs::write(&path, old).unwrap();
    let mut library = Library::open(path.clone());
    assert_eq!(library.error, 0);
    assert!(!library.entries[0].is_template);
    assert_eq!(std::fs::read(&path).unwrap(), old);
    let mut entries = library.entries.clone();
    entries[0].is_template = true;
    assert_eq!(library.save(1, &entries), 0);
    assert!(load(&path).unwrap()[0].is_template);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["version"], 2);
    entries[0].command = "echo {{invalid".into();
    assert_eq!(library.save(2, &entries), 2);
    assert!(load(&path).unwrap() == library.entries);
}
