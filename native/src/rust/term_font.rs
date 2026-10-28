//! One installed font face supplies both Rust grid metrics and Qt's family.
use fontdb::{Database, Family, Query, Source, Style, Weight};
use std::sync::OnceLock;

pub(super) struct Selection {
    pub path: String,
    pub family: String,
    pub index: u32,
}

pub(super) fn selected() -> &'static Selection {
    static FONT: OnceLock<Selection> = OnceLock::new();
    FONT.get_or_init(|| {
        let mut database = Database::new();
        database.load_system_fonts();
        let families: &[Family<'_>] = if cfg!(windows) {
            &[
                Family::Name("Cascadia Mono"),
                Family::Name("Consolas"),
                Family::Monospace,
            ]
        } else {
            &[Family::Monospace]
        };
        let matched = database
            .query(&Query {
                families,
                ..Query::default()
            })
            .and_then(|id| database.face(id))
            .filter(|face| face.monospaced);
        let face = matched
            .or_else(|| {
                database.faces().find(|face| {
                    face.monospaced && face.style == Style::Normal && face.weight == Weight::NORMAL
                })
            })
            .expect("No installed monospace font; install a platform font package");
        let path = match &face.source {
            Source::File(path) | Source::SharedFile(path, _) => path,
            Source::Binary(_) => unreachable!("system font discovery returns file sources"),
        };
        Selection {
            path: path.to_string_lossy().into_owned(),
            family: face
                .families
                .first()
                .expect("installed font family")
                .0
                .clone(),
            index: face.index,
        }
    })
}
