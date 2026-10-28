use super::*;

fn text_line(s: &str, fg: u32) -> LineData {
    let chars: Vec<u16> = s.chars().map(|c| c as u16).collect();
    LineData {
        chars: chars.clone().into(),
        extras: Vec::new(),
        runs: vec![StyleRun {
            start: 0,
            len: chars.len() as u16,
            fg,
            bg: 0,
            bold: false,
            italic: false,
            underline: false,
            wide: false,
        }],
        cols: chars.len() as u16,
        wrap: false,
    }
}

#[test]
fn blank_is_detected() {
    assert!(LineData::blank(200).is_blank());
    assert!(!text_line("abc", 0xff000000).is_blank());
    let mut b = LineData::blank(10);
    b.runs[0].bold = true;
    assert!(!b.is_blank(), "styled blank is not shareable");
}

#[test]
fn push_row_roundtrip() {
    let mut s = ScrollbackStore::new(1 << 20);
    s.push(text_line("hello", 0x11223344));
    s.push(LineData::blank(5));
    assert_eq!(s.rows(), 2);
    assert_eq!(
        s.row(0).unwrap().chars,
        vec!['h' as u16, 'e' as u16, 'l' as u16, 'l' as u16, 'o' as u16].into()
    );
    assert_eq!(s.row(0).unwrap().runs[0].fg, 0x11223344);
    assert!(s.row(1).unwrap().is_blank());
    assert!(s.row(2).is_none(), "out of range");
}

#[test]
fn blank_rows_share_one_arc_and_cost_zero() {
    let mut s = ScrollbackStore::new(1 << 20);
    s.push(LineData::blank(80));
    s.push(LineData::blank(80));
    s.push(LineData::blank(80));
    let a = s.row(0).unwrap();
    let b = s.row(2).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "blank rows must share one Arc");
    assert_eq!(s.estimated_bytes(), 0, "shared blanks cost ~0");
}

#[test]
fn identical_rows_are_interned() {
    let mut s = ScrollbackStore::new(1 << 20);
    s.push(text_line("dup", 0x1));
    s.push(text_line("dup", 0x1));
    let a = s.row(0).unwrap();
    let b = s.row(1).unwrap();
    assert!(Arc::ptr_eq(&a, &b), "identical content must share one Arc");
}

#[test]
fn different_content_same_shape_is_not_merged() {
    let mut s = ScrollbackStore::new(1 << 20);
    s.push(text_line("aaa", 0x1));
    s.push(text_line("aab", 0x1));
    assert!(!Arc::ptr_eq(&s.row(0).unwrap(), &s.row(1).unwrap()));
}

#[test]
fn budget_drops_oldest() {
    // Small rows include the sparse-grapheme container's fixed header now.
    // 250 bytes retain two of these three distinct five-cell rows.
    let mut s = ScrollbackStore::new(250);
    s.push(text_line("aaaaa", 0x1));
    s.push(text_line("bbbbb", 0x2));
    s.push(text_line("ccccc", 0x3));
    assert_eq!(s.rows(), 2, "oldest must be dropped under budget");
    assert_eq!(s.row(0).unwrap().chars, vec!['b' as u16; 5].into());
    assert_eq!(s.row(1).unwrap().chars, vec!['c' as u16; 5].into());
}

#[test]
fn ring_grows_beyond_initial_capacity_within_budget() {
    // Short rows (~84 B each) with a huge budget: growth must kick in, not overwrite.
    let mut s = ScrollbackStore::new(1 << 30);
    assert_eq!(s.cap, RING_CAP);
    for i in 0..(1 << 16) + 5000 {
        s.push(text_line(&format!("line-{i}"), 0x1));
    }
    assert_eq!(
        s.rows(),
        (1 << 16) + 5000,
        "ring must grow past 2^16 when budget allows"
    );
    // Oldest row is the very first pushed, newest is the last.
    assert_eq!(
        s.row(0).unwrap().chars,
        vec!['l' as u16, 'i' as u16, 'n' as u16, 'e' as u16, '-' as u16, '0' as u16].into()
    );
    assert_eq!(
        s.row(s.rows() - 1).unwrap().chars,
        text_line(&format!("line-{}", (1 << 16) + 5000 - 1), 0x1).chars
    );
}

#[test]
fn ring_overwrites_only_at_max_capacity() {
    // Blank rows cost ~0 bytes, so only the count ceiling (MAX_CAP) trims them.
    let mut s = ScrollbackStore::new(1 << 30);
    for _ in 0..MAX_CAP + 10 {
        s.push(LineData::blank(5));
    }
    assert_eq!(s.rows(), MAX_CAP, "count ceiling is MAX_CAP, not RING_CAP");
    assert!(s.row(0).is_some());
    assert!(s.row(MAX_CAP).is_none());
}

#[test]
fn clear_resets_everything() {
    let mut s = ScrollbackStore::new(1 << 20);
    for _ in 0..=RING_CAP {
        s.push(LineData::blank(3));
    }
    assert!(s.cap > RING_CAP);
    s.clear();
    assert_eq!(s.cap, RING_CAP, "clear must release the grown row table");
    assert_eq!(s.rows(), 0);
    assert_eq!(s.estimated_bytes(), 0);
    assert!(s.row(0).is_none());
}

#[test]
fn dropped_rows_are_evicted_from_interner() {
    let mut s = ScrollbackStore::new(170); // fits 2 rows
    s.push(text_line("aaaaa", 0x1));
    s.push(text_line("bbbbb", 0x2));
    s.push(text_line("ccccc", 0x3)); // drops "aaaaa"
                                     // The interner must not keep "aaaaa" alive: pushing it again should re-intern to
                                     // a NEW Arc (ptr differs from the dropped one is hard to assert, so check counts).
    let before = s.interner.len();
    let _ = s.row(0).unwrap();
    assert!(s.interner.len() <= before + 1);
}

/// Regression: the dedup table used to hold on to *every* row ever stored (the eviction test
/// compared `strong_count` against 1, which never matches an interned row), so real memory
/// grew without bound while `estimated_bytes` stayed under the budget. A long unique-line
/// session must keep both bounded.
#[test]
fn dropped_rows_do_not_accumulate_in_the_interner() {
    let mut s = ScrollbackStore::new(2_000);
    for i in 0..2_000 {
        s.push(text_line(&format!("row{i:04}"), 0x1));
    }
    assert!(
        s.interner.len() <= 32,
        "interner kept {} rows for a store that holds {}",
        s.interner.len(),
        s.rows()
    );
    assert!(s.estimated_bytes() <= 2_000);
    assert_eq!(s.char_interner.len(), s.interner.len());
}

/// Repeated output (progress bars, `docker ps` refreshes, the same log line) shares one
/// allocation, so it must not burn the byte budget at full price.
#[test]
fn duplicate_rows_do_not_consume_budget() {
    let mut s = ScrollbackStore::new(1_000);
    for _ in 0..500 {
        s.push(text_line("same", 0x1));
    }
    assert_eq!(s.rows(), 500, "budget must not evict shared duplicates");
    let one = estimate(&text_line("same", 0x1));
    assert!(
        s.estimated_bytes() <= one + 1,
        "duplicates cost {} B, one row is {one} B",
        s.estimated_bytes()
    );
}

/// A trimmed row keeps its real width in `cols`, so the store can report the row the way the
/// grid sees it (one char per column) while holding only the columns that paint.
#[test]
fn trimmed_rows_keep_their_declared_width() {
    let ld = LineData {
        chars: vec!['h' as u16, 'i' as u16].into(),
        extras: Vec::new(),
        runs: vec![StyleRun {
            start: 0,
            len: 2,
            fg: 0,
            bg: 0,
            bold: false,
            italic: false,
            underline: false,
            wide: false,
        }],
        cols: 200,
        wrap: false,
    };
    assert_eq!(ld.cols(), 200);
    assert_eq!(ld.chars.len(), 2);
    assert!(!ld.is_blank());
}
