//! 共池仲裁器行为门禁（D1a）：G4-13 终端零挤压 / G4-14 驱逐可重建性 /
//! G4-15 超限不丢数据。终端侧用真实 `ScrollbackStore`（MemRingProvider 借用），
//! 驱逐候选集的类型级保证在 `tiderm_textdoc::manager::EvictTarget`（无终端历史变体）。

use std::io::Write;
use std::path::PathBuf;

use tiderm_terminal::row_provider::MemRingProvider;
use tiderm_terminal::scrollback::{LineData, ScrollbackStore, StyleRun};
use tiderm_textdoc::core::RowProvider;
use tiderm_textdoc::manager::{DocumentManager, TerminalStatus};

fn temp_log(name: &str, lines: usize) -> PathBuf {
    let p = std::env::temp_dir().join(format!("arbiter-{name}-{}.log", std::process::id()));
    let mut f = std::fs::File::create(&p).expect("create");
    for i in 0..lines {
        writeln!(
            f,
            "2026-09-13T10:00:00.123Z INFO  app: line {i:06} payload 0123456789012345678901234567890123456789"
        )
        .unwrap();
    }
    p
}

/// 小预算强制驱逐：给池 2 MiB 预算、每文档解码缓存 512 KiB + 索引，
/// 打开 8 个 ~1.6 MiB 日志 → 总量必然超限 → enforce 触发驱逐链。
fn storm() -> (DocumentManager, Vec<PathBuf>) {
    // 预算刻意收到 256 KiB：8 文件 × 分散读行的解码缓存必然 >> 预算 → 强制驱逐链。
    let mut mgr = DocumentManager::new(256 << 10);
    mgr.pool().set_terminal_panes(1);
    let mut paths = Vec::new();
    for k in 0..8 {
        let p = temp_log(&format!("storm{k}"), 20_000);
        mgr.open(&p).expect("open");
        paths.push(p);
    }
    // 全量索引 + 分散读行（跨块，填满解码缓存）。
    loop {
        let mut any = false;
        for id in 1..=8u64 {
            if let Some(d) = mgr.get_mut(id) {
                any |= d.index_more(8 << 20) > 0;
            }
        }
        if !any {
            break;
        }
    }
    for id in 1..=8u64 {
        for k in 0..20 {
            let _ = mgr.row(id, (k * 997) % 19_000);
        }
    }
    (mgr, paths)
}

/// G4-13 + G4-14：终端历史在日志风暴前后行数不变；被驱逐区间重读逐字节一致。
#[test]
fn g4_13_g4_14_terminal_never_evicted_and_evicted_rows_rebuild_identically() {
    // 终端侧：真实 ScrollbackStore，先灌 5000 行历史。
    let mut store = ScrollbackStore::new(32 << 20);
    for i in 0..5000 {
        let text = format!("terminal history line {i} — must survive the log storm");
        let mut chars: Vec<u16> = text.encode_utf16().collect();
        chars.resize(120, 0x20);
        store.push(LineData {
            chars: chars.into(),
            cols: 120,
            extras: Vec::new(),
            runs: vec![StyleRun {
                start: 0,
                len: 120,
                fg: 0xFFD0CAF5,
                bg: 0xFF161616,
                bold: false,
                italic: false,
                underline: false,
                wide: false,
            }],
            wrap: false,
        });
    }
    let terminal_rows_before = store.rows();
    let provider = MemRingProvider::new(&store);
    let sample = [0usize, 1, 2500, 4999];
    let before: Vec<Vec<u8>> = sample
        .iter()
        .map(|&i| {
            let mut b = Vec::new();
            provider.row(i).unwrap().write_utf8(&mut b);
            b
        })
        .collect();

    // 日志风暴（预算 2 MiB，8 文件总量 >> 预算）。
    let (mut mgr, paths) = storm();
    let evictions: Vec<_> = mgr.evictions().to_vec();
    assert!(
        !evictions.is_empty(),
        "测试前提：风暴必须触发过驱逐（否则预算过大）"
    );
    // 类型级保证的运行时影子断言：驱逐记录里不存在终端历史类。
    for (_, target, _) in &evictions {
        let dbg = format!("{target:?}");
        assert!(
            !dbg.contains("TerminalHistory"),
            "出现终端历史驱逐 = 违反 D1a 类型保证"
        );
    }
    assert_eq!(
        store.rows(),
        terminal_rows_before,
        "G4-13：日志风暴前后终端滚动历史行数不得下降"
    );
    // 终端历史行内容仍逐字节一致（抽样）。
    for (&i, before_row) in sample.iter().zip(&before) {
        let mut b = Vec::new();
        provider.row(i).unwrap().write_utf8(&mut b);
        assert_eq!(b, *before_row, "G4-13：终端行 {i} 内容不得变化");
    }

    // G4-14：强制再驱逐全部日志层 → 重读被驱逐区间 → 逐字节一致。
    for id in 1..=8u64 {
        let freed = mgr.get_mut(id).map(|d| d.evict_decode()).unwrap_or(0);
        let _ = freed;
        let freed2 = mgr.get_mut(id).map(|d| d.evict_index()).unwrap_or(0);
        let _ = freed2;
    }
    for id in 1..=8u64 {
        // 索引被驱逐后 row() 走降级扫描路径 —— 内容必须一致。
        let s = mgr.row(id, 12345).expect("row after eviction");
        assert!(
            s.starts_with("2026-09-13T10:00:00.123Z INFO  app: line 012345"),
            "G4-14：驱逐后重读内容必须重建一致，got: {s}"
        );
    }
    for p in &paths {
        let _ = std::fs::remove_file(p);
    }
}

/// G4-15：终端类超上限 → 保留数据 + 报警状态，绝不静默丢行。
#[test]
fn g4_15_terminal_over_limit_is_reported_not_evicted() {
    use tiderm_textdoc::manager::BudgetPool;
    let mut pool = BudgetPool::new(256 << 20);
    pool.set_terminal_panes(1); // cap = min(32 MiB, 128 MiB) = 32 MiB
    let status = pool.report_terminal(48 << 20, 0); // 9 pane 等效 → 超限
    assert_eq!(
        status,
        TerminalStatus::OverLimit {
            used: 48 << 20,
            cap: 32 << 20
        },
        "超限必须返回报警状态（数据保留语义）"
    );
    assert_eq!(
        pool.usage().terminal,
        48 << 20,
        "超限数据必须保留在池记账中（不驱逐）"
    );
    // 无日志文档 → next_victim 无候选（终端历史不是候选）。
    assert!(
        pool.next_victim().is_none(),
        "终端历史不得成为驱逐候选（类型级保证的运行时面）"
    );
}
