//! MemRingProvider 的一致性契约测试（G4-11 的另一半；FileChunkProvider 侧在
//! `tiderm-textdoc/tests/contract_file.rs`）。两端跑 **同一套** 规范用例 ——
//! 这就是 D5「统一抽象」的入场券。

use tiderm_terminal::row_provider::MemRingProvider;
use tiderm_terminal::scrollback::{LineData, ScrollbackStore, StyleRun};
use tiderm_textdoc::contract::{assert_contract, canonical_fixture};

/// 把规范行集（UTF-8）喂进 ScrollbackStore：每行 = 一个 LineData（UTF-16 列 +
/// 覆盖全宽的单一样式 run）。空白行用 `LineData::blank`（is_blank 归一路径）。
fn store_with(rows: &[Vec<u8>], cols: usize) -> ScrollbackStore {
    let mut store = ScrollbackStore::new(32 << 20);
    for row in rows {
        let ld = if row.is_empty() {
            LineData::blank(cols)
        } else {
            let text = String::from_utf8_lossy(row);
            let mut chars: Vec<u16> = text.encode_utf16().collect();
            chars.resize(cols, 0x20); // 补空格到全宽（列存储语义）
            LineData {
                chars: chars.into(),
                extras: Vec::new(),
                runs: vec![StyleRun {
                    start: 0,
                    len: cols as u16,
                    fg: 0xFFD0CAF5, // little-endian packed RGBA（值不影响契约）
                    bg: 0xFF161616,
                    bold: false,
                    italic: false,
                    underline: false,
                    wide: false,
                }],
                cols: cols as u16,
                wrap: false,
            }
        };
        store.push(ld);
    }
    store
}

#[test]
fn mem_ring_provider_satisfies_the_contract() {
    let fixture = canonical_fixture();
    // 网格宽度 = 最长规范行 + 2（终端行是定宽列存储；4 KiB 长行必须完整放进
    // 一行 —— 真实终端里那是 wrap 成多行，但本契约比较的是"行来源"，非布局）。
    let cols = fixture.iter().map(|r| r.len()).max().unwrap_or(80) + 2;
    let store = store_with(&fixture, cols);
    assert_contract(&MemRingProvider::new(&store), &fixture);
}

#[test]
fn empty_store_satisfies_the_contract() {
    let store = ScrollbackStore::new(32 << 20);
    assert_contract(&MemRingProvider::new(&store), &[]);
}
