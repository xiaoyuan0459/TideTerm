//! FileChunkProvider 的一致性契约测试（G4-11 的一半；另一半在 widget 的
//! `tests/row_provider_contract.rs`，跑 `MemRingProvider`）。

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use tiderm_textdoc::contract::{assert_contract, canonical_fixture};
use tiderm_textdoc::core::RowProvider;
use tiderm_textdoc::provider::LogDocument;

fn temp_log(name: &str, rows: &[Vec<u8>]) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tiderm-textdoc-{}-{}-{}.log",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let mut f = std::fs::File::create(&p).expect("create temp log");
    for (i, row) in rows.iter().enumerate() {
        f.write_all(row).unwrap();
        if i + 1 < rows.len() {
            f.write_all(b"\n").unwrap();
        }
    }
    p
}

#[test]
fn file_provider_satisfies_the_contract() {
    let fixture = canonical_fixture();
    let path = temp_log("contract", &fixture);
    let mut doc = LogDocument::open(&path).expect("open");
    doc.index_more(u64::MAX); // 全量索引（契约需要全部行可见）
    assert_contract(&doc, &fixture);
    let _ = Command::new("cmd").args(["/C", "del"]).arg(&path).output();
}

#[test]
fn file_provider_is_lazy_and_incremental() {
    // G4-1/G4-2 的机制性验证：文件大于首窗（FIRST_INDEX_WINDOW = 2 MiB）时，
    // 打开只索引前窗，行数随 index_more 增长。
    let path = temp_log("lazy", &[]);
    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append");
        // ~16 MiB 结构化日志（> 2 MiB 首窗）。
        let line = b"2026-09-13T10:00:00.123Z INFO  app: lazy-index proof line with payload 0123456789\r\n";
        for i in 0..200_000u32 {
            let _ = writeln!(
                f,
                "{}{}",
                String::from_utf8_lossy(&line[..line.len() - 2]),
                i % 1000
            );
        }
    }
    let mut doc = LogDocument::open(&path).expect("open");
    let first_screen_rows = doc.rows();
    assert!(
        first_screen_rows >= 1,
        "打开后首窗必须已有可读行（首屏不空）"
    );
    assert_eq!(doc.state(), tiderm_textdoc::provider::DocState::Indexing);
    let mid = doc.rows();
    doc.index_more(4 << 20);
    assert!(doc.rows() > mid, "index_more 必须推进已知行数");
    doc.index_more(u64::MAX);
    assert_eq!(doc.rows(), 200_000, "全量索引后行数必须完整");
    assert_eq!(doc.state(), tiderm_textdoc::provider::DocState::Ready);
    // 首屏不整读：打开阶段读盘量 ≪ 文件大小。
    let io = doc.io_bytes.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        io < doc.source_len().unwrap_or(0) / 4,
        "打开阶段读盘 {io} 字节，必须远小于文件总量（不整读）"
    );
    let _ = Command::new("cmd").args(["/C", "del"]).arg(&path).output();
}

#[test]
fn file_provider_last_line_without_newline_counts() {
    let fixture = vec![b"alpha".to_vec(), b"beta".to_vec()];
    let path = temp_log("nonl", &fixture);
    let mut doc = LogDocument::open(&path).expect("open");
    doc.index_more(u64::MAX);
    assert_contract(&doc, &fixture);
    let _ = Command::new("cmd").args(["/C", "del"]).arg(&path).output();
}

#[test]
fn file_provider_empty_file_has_zero_rows() {
    let path = temp_log("empty", &[]);
    let mut doc = LogDocument::open(&path).expect("open");
    doc.index_more(u64::MAX);
    assert_contract(&doc, &[]);
    let _ = Command::new("cmd").args(["/C", "del"]).arg(&path).output();
}
