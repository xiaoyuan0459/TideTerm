//! Validated default rule storage. Callers choose manual or opted-in close writes.
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tiderm_terminal::{
    marks::{self, MarkSpec},
    search::SearchOptions,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    version: u32,
    rules: Vec<SavedRule>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRule {
    pattern: String,
    flags: u32,
    color: u8,
    enabled: bool,
}

fn path() -> Result<PathBuf, String> {
    #[cfg(test)]
    {
        // Native unit tests must never read or write the real desktop profile.
        static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        let dir = ROOT.get_or_init(|| tempfile::tempdir().expect("isolated mark test directory"));
        Ok(dir.path().join("text-marks.json"))
    }
    #[cfg(not(test))]
    {
        tiderm_core::config_dir().map(|dir| dir.join("text-marks.json"))
    }
}
pub(super) fn load() -> Result<Vec<MarkSpec>, String> {
    load_from(&path()?)
}
pub(super) fn save(specs: &[MarkSpec]) -> Result<(), String> {
    save_to(&path()?, specs)
}

fn load_from(path: &Path) -> Result<Vec<MarkSpec>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("read mark defaults".into()),
    };
    let mut bytes = Vec::new();
    file.take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "read mark defaults")?;
    if bytes.len() > 32768 {
        return Err("mark defaults exceed size limit".into());
    }
    let profile: Profile = serde_json::from_slice(&bytes).map_err(|_| "invalid mark defaults")?;
    if profile.version != 1 || profile.rules.iter().any(|r| r.flags > 7) {
        return Err("unsupported mark defaults".into());
    }
    let rules: Vec<_> = profile
        .rules
        .into_iter()
        .map(|r| MarkSpec {
            pattern: r.pattern,
            options: SearchOptions::from_flags(r.flags),
            color: r.color,
            enabled: r.enabled,
        })
        .collect();
    marks::validate_rules(&rules).map_err(|_| "invalid mark rules")?;
    Ok(rules)
}
fn save_to(path: &Path, specs: &[MarkSpec]) -> Result<(), String> {
    marks::validate_rules(specs).map_err(|_| "invalid mark rules")?;
    let rules = specs
        .iter()
        .map(|r| SavedRule {
            pattern: r.pattern.clone(),
            flags: r.options.to_flags(),
            color: r.color,
            enabled: r.enabled,
        })
        .collect();
    let bytes = serde_json::to_vec_pretty(&Profile { version: 1, rules })
        .map_err(|_| "encode mark defaults")?;
    if bytes.len() > 32768 {
        return Err("mark defaults exceed size limit".into());
    }
    let parent = path.parent().ok_or("mark defaults directory unavailable")?;
    std::fs::create_dir_all(parent).map_err(|_| "create mark defaults directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "create mark defaults temporary file")?;
    file.write_all(&bytes).map_err(|_| "write mark defaults")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "flush mark defaults")?;
    file.persist(path).map_err(|_| "replace mark defaults")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_round_trip_replaces_atomically_and_rejects_invalid_profiles_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("marks.json");
        let rules = vec![MarkSpec {
            pattern: "中e\u{301}|WARN".into(),
            options: SearchOptions::from_flags(3),
            color: 4,
            enabled: false,
        }];
        save_to(&target, &rules).unwrap();
        assert_eq!(load_from(&target).unwrap(), rules);
        let original = std::fs::read(&target).unwrap();
        let mut invalid = rules.clone();
        invalid[0].pattern = "(".into();
        assert!(save_to(&target, &invalid).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), original);
        save_to(&target, &[]).unwrap();
        assert!(load_from(&target).unwrap().is_empty());
        for broken in [
            br#"{"version":2,"rules":[]}"#.as_slice(),
            br#"{"version":1,"rules":[{"pattern":"(","flags":2,"color":0,"enabled":true}]}"#,
        ] {
            std::fs::write(&target, broken).unwrap();
            assert!(load_from(&target).is_err());
            assert_eq!(std::fs::read(&target).unwrap(), broken);
        }
        std::fs::write(&target, vec![b' '; 32769]).unwrap();
        assert!(load_from(&target).is_err());
    }

    #[test]
    fn close_saves_changed_rules_without_clobbering_other_panes_or_partial_batch_writes() {
        use crate::rust::mark_persistence::*;
        use crate::rust::{term, term_marks::*, term_ops, term_tests};
        let _guard = term_tests::input_lock_guard();
        let original = load().unwrap();
        save(&[]).unwrap();
        let a = term::term_new_tab();
        let b = term::term_new_tab();
        assert_eq!(term_mark_put(a, 0, "AUTO_A", 1, 2, true).error, 0);
        assert_eq!(term_mark_prepare_close(&[a]).error, 0);
        assert!(load().unwrap().is_empty(), "manual mode never writes");
        assert_eq!(term_mark_set_save_on_close(a, true).error, 0);
        assert!(
            crate::bridge::session::sess_active_config(a)
                .options
                .mark_save_on_close
        );
        assert_eq!(term_mark_set_save_on_close(b, true).error, 0);
        assert_eq!(term_mark_put(b, 0, "AUTO_B", 1, 3, true).error, 0);
        assert_eq!(term_mark_prepare_close(&[a, b]).error, 11);
        assert!(load().unwrap().is_empty(), "no partial batch save");
        assert_eq!(term_mark_set_save_on_close(b, false).error, 0);
        term_ops::term_set_locked(true);
        assert_eq!(
            term_mark_prepare_close(&[b]).error,
            0,
            "manual close needs no hidden-rule save while locked"
        );
        let locked = [
            term_mark_prepare_close(&[a]).error,
            term_mark_set_save_on_close(a, false).error,
        ];
        term_ops::term_set_locked(false);
        assert_eq!(locked, [6, 6]);
        assert_eq!(term_mark_set_save_on_close(b, true).error, 0);
        assert_eq!(term_mark_prepare_close(&[a]).error, 0);
        let future = term::term_new_tab();
        assert_eq!(term_mark_rows(future)[0].pattern, "AUTO_A");
        assert_eq!(
            term_mark_rows(b)[0].pattern,
            "AUTO_B",
            "live rules stay pane owned"
        );
        assert_eq!(
            term_mark_prepare_close(&[b]).error,
            11,
            "stale baseline cannot overwrite"
        );
        assert_eq!(
            term_mark_defaults(b, true).error,
            0,
            "explicit save chooses the winner"
        );
        assert_eq!(
            term_mark_prepare_close(&[a]).error,
            0,
            "unchanged pane cannot clobber newer defaults"
        );
        assert_eq!(load().unwrap()[0].pattern, "AUTO_B");
        assert_eq!(term_mark_clear(b).error, 0);
        assert_eq!(
            term_mark_prepare_close(&[b]).error,
            0,
            "clearing rules is a real saved edit"
        );
        assert!(load().unwrap().is_empty());
        // A damaged file rejects close without overwriting it.
        assert_eq!(term_mark_put(b, 0, "RETRY", 1, 1, true).error, 0);
        std::fs::write(path().unwrap(), b"broken profile").unwrap();
        assert_eq!(term_mark_prepare_close(&[b]).error, 8);
        assert_eq!(std::fs::read(path().unwrap()).unwrap(), b"broken profile");
        let damaged = term::term_new_tab();
        assert_eq!(term_mark_set_save_on_close(damaged, true).error, 0);
        assert_eq!(term_mark_defaults(b, true).error, 0);
        assert_eq!(
            term_mark_prepare_close(&[damaged]).error,
            11,
            "pane without a loaded baseline cannot erase another pane's repair"
        );
        assert_eq!(load().unwrap()[0].pattern, "RETRY");
        for id in [a, b, future, damaged] {
            term::term_close_tab(id);
        }
        assert_eq!(term_mark_prepare_close(&[a]).error, 7);
        assert_eq!(term_mark_set_save_on_close(a, false).error, 7);
        save(&original).unwrap();
    }
}
