//! G4-11 Provider 一致性套件 —— **入场券，非可选**（ADR-013 §Decision 2 硬约束 3）。
//!
//! `MemRingProvider`（widget）/ `FileChunkProvider`（本 crate）/ `StreamProvider`
//! （后续）是**同一契约的三个实现**。本模块提供与实现无关的规范性用例：
//! 每个实现在自己的测试里构造**同一份规范行集**，然后调用
//! [`assert_contract`] —— 行内容逐字节一致（以 UTF-8 为归一口径）、边界、
//! 重复读稳定性。
//!
//! 新增 provider 时：把规范行集写入你的存储，调 [`assert_contract`]。
//! 若该 provider 支持驱逐，另加「驱逐后重读逐字节一致」用例（G4-14 模板见
//! 探针 `poc/logstore-probe`）。

use crate::core::{RowProvider, RowRef};

/// 规范行集：覆盖空白行、CJK、超长行（4 KiB）、重复行（interner 压力）、
/// 特殊字节。全部 BMP 内 UTF-8/UTF-16 可无损互转（终端列存储的口径）。
pub fn canonical_fixture() -> Vec<Vec<u8>> {
    let mut rows: Vec<Vec<u8>> = Vec::new();
    rows.push(b"2026-09-13T10:00:00.123Z INFO  main: service started (pid=4242)".to_vec());
    rows.push(Vec::new()); // 空白行 —— 最常见行
    rows.push(
        "2026-09-13T10:00:00.200Z ERROR db: 连接超时 —— 重试 3 次后放弃"
            .as_bytes()
            .to_vec(),
    );
    // 超长行（4 KiB）：跨块拼接路径的触发器。
    let mut long = b"2026-09-13T10:00:00.300Z WARN  big: ".to_vec();
    long.extend(std::iter::repeat_n(b'x', 4096));
    rows.push(long);
    // 重复行 ×3（interner / 去重压力；WindTerm 有样式档 8.5× 压缩的主力形态）。
    for _ in 0..3 {
        rows.push(b"    at com.tideterm.app.Session.run(Session.java:101)".to_vec());
    }
    rows.push(
        "2026-09-13T10:00:01.000Z DEBUG io: bytes=65536 latency=2.9ms ✓"
            .as_bytes()
            .to_vec(),
    );
    rows
}

/// `RowRef` → 规范 UTF-8 字节。
pub fn row_bytes(r: RowRef) -> Vec<u8> {
    let mut b = Vec::new();
    r.write_utf8(&mut b);
    b
}

/// 行内容的**显著口径**：去行尾空白。终端列模型的行是定宽补空格的
/// （`LineData.chars` 补至网格宽度），行尾空格不承载信息 —— 与编辑器/日志的
/// 行级比较惯例一致。
pub fn normalize(mut row: Vec<u8>) -> Vec<u8> {
    while matches!(row.last(), Some(b' ') | Some(b'\t')) {
        row.pop();
    }
    row
}

/// 契约断言（全部实现必须全绿 —— G4-11）。
pub fn assert_contract(p: &dyn RowProvider, fixture: &[Vec<u8>]) {
    assert_eq!(p.rows(), fixture.len(), "rows() 必须等于规范行集长度");

    // 1) 逐行内容一致（UTF-8 归一 + 去行尾空白）。
    for (i, want) in fixture.iter().enumerate() {
        let r = p.row(i).unwrap_or_else(|| panic!("row({i}) 必须存在"));
        let got = normalize(row_bytes(r));
        let want = normalize(want.clone());
        assert_eq!(
            got,
            want,
            "row({i}) 内容不一致（UTF-8 归一口径）\n got: {:?}\nwant: {:?}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(&want)
        );
    }

    // 2) 越界必须 None。
    assert!(p.row(fixture.len()).is_none(), "越界行必须返回 None");
    assert!(p.row(usize::MAX / 2).is_none(), "远越界必须返回 None");

    // 3) 重复读稳定（同一行两次读内容一致；驱逐 / 重载不得改变内容）。
    if !fixture.is_empty() {
        let a = row_bytes(p.row(0).expect("row(0)"));
        let b = row_bytes(p.row(0).expect("row(0) again"));
        assert_eq!(a, b, "row(0) 两次读取必须一致");
        let last = fixture.len() - 1;
        let a = row_bytes(p.row(last).expect("last row"));
        let b = row_bytes(p.row(last).expect("last row again"));
        assert_eq!(a, b, "末行两次读取必须一致");
    }
}
