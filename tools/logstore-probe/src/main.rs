//! G4-A 探针（ADR-013）—— 语料生成 + 闸门测量。
//!
//! 子命令：
//!   gen-corpus <path> <MiB>   生成结构化日志语料（时间戳/级别/重复栈帧/CJK/JSON 块）
//!   bench <corpus-path>       跑 G4-2/3/5/6/7/10 并打印判据表
//!
//! 判据口径全部来自 ADR-013 §验收判据；估算值允许按实测校正，D1 确认值不可下调。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tiderm_textdoc::core::RowProvider;
use tiderm_textdoc::manager::DocumentManager;
use tiderm_textdoc::tier::{compress_cold, COLD_BLOCK};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("gen-corpus") => {
            let path = args.get(2).expect("usage: gen-corpus <path> <MiB>");
            let mib: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1024);
            gen_corpus(Path::new(path), mib << 20);
        }
        Some("bench") => {
            let path = args.get(2).expect("usage: bench <corpus-path>");
            bench(Path::new(path));
        }
        _ => {
            eprintln!("usage: logstore-probe gen-corpus <path> <MiB> | bench <corpus-path>");
            std::process::exit(2);
        }
    }
}

/// 结构化日志语料：时间戳 + 级别 + logger + 消息族（请求、重复栈帧、JSON 块、
/// CJK 行）。平均 ~100 B/行。返回行数。
fn gen_corpus(path: &Path, target_bytes: u64) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).expect("mkdir corpus dir");
    }
    let mut f = std::io::BufWriter::with_capacity(
        1 << 20,
        std::fs::File::create(path).expect("create corpus"),
    );
    let mut written: u64 = 0;
    let mut seq: u64 = 0;
    let levels = ["INFO", "WARN", "ERROR", "DEBUG"];
    let loggers = [
        "api.server",
        "db.pool",
        "auth.session",
        "sftp.transfer",
        "ai.router",
    ];
    let cjk = [
        "连接超时 —— 已重试",
        "会话已建立，握手完成",
        "配额告警：用量超过阈值",
    ];
    let mut stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis();
    while written < target_bytes {
        seq += 1;
        stamp += 7;
        let level = levels[(seq % 4) as usize];
        let logger = loggers[(seq % 5) as usize];
        let line = match seq % 10 {
            0..=2 => {
                // 重复栈帧（interner / 温层压缩的主力形态）。
                format!(
                    "{ts} {level} {logger}: at com.tideterm.app.Session.run(Session.java:{})\n",
                    seq % 9973,
                    ts = fmt_stamp(stamp)
                )
            }
            3 => format!(
                "{ts} {level} {logger}: 请求处理完成 rid=req-{seq} bytes={} latency={}.{}ms\n",
                seq % 65536,
                seq % 90 + 1,
                seq % 10,
                ts = fmt_stamp(stamp)
            ),
            5 => format!(
                "{ts} ERROR {}: {}\n",
                logger,
                cjk[seq as usize % cjk.len()],
                ts = fmt_stamp(stamp)
            ),
            7 => format!(
                "{ts} DEBUG {logger}: {{\"rid\":\"req-{seq}\",\"ok\":true,\"n\":{},\"tags\":[\"a\",\"b\"]}}\n",
                seq % 1024,
                ts = fmt_stamp(stamp)
            ),
            _ => format!(
                "{ts} {level} {logger}: heartbeat seq={seq} ok\n",
                ts = fmt_stamp(stamp)
            ),
        };
        written += line.len() as u64;
        f.write_all(line.as_bytes()).expect("write corpus");
    }
    f.flush().expect("flush corpus");
    eprintln!(
        "corpus: {} ({} MiB, {} lines)",
        path.display(),
        written >> 20,
        seq
    );
}

fn fmt_stamp(ms: u128) -> String {
    let secs = (ms / 1000) as u64;
    let frac = ms % 1000;
    // 简化 UTC 格式化（避免 chrono 依赖）。
    let days = secs / 86400;
    let rem = secs % 86400;
    format!(
        "2026-09-{}T{:02}:{:02}:{:02}.{:03}Z",
        13 + (days % 15) as u32,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        frac
    )
}

struct SmallFile {
    path: PathBuf,
}

impl Drop for SmallFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn bench(corpus: &Path) {
    println!("=== G4-A 闸门测量（ADR-013 §验收判据） ===");
    let file_len = std::fs::metadata(corpus).expect("corpus len").len();

    // ---- G4-1/G4-2：打开不整读 + 首屏 ≤500 ms ----
    let t0 = Instant::now();
    let mut doc = tiderm_textdoc::LogDocument::open(corpus).expect("open corpus");
    let open_io = doc.io_bytes.load(std::sync::atomic::Ordering::Relaxed);
    // 首屏：读前 50 行。
    let mut first_screen_bytes = 0usize;
    for i in 0..50 {
        let r = doc.row(i).expect("first-screen row");
        first_screen_bytes += tiderm_textdoc::contract::row_bytes(r).len();
    }
    let first_screen = t0.elapsed();
    let g4_1 = open_io < file_len / 10; // 打开读盘 < 文件 10%
    let g4_2 = first_screen.as_millis() <= 500;
    println!(
        "G4-1 单文件不整读     : {}（打开读盘 {} B / 文件 {} B）",
        pass(g4_1),
        open_io,
        file_len
    );
    println!(
        "G4-2 首屏 ≤500 ms     : {}（{:?}，{} 行 {} B）",
        pass(g4_2),
        first_screen,
        50,
        first_screen_bytes
    );

    // ---- 全量索引（后台视角）+ G4-10：索引内存占比 ----
    let t = Instant::now();
    loop {
        if doc.index_more(64 << 20) == 0 {
            break;
        }
    }
    let index_done = t.elapsed();
    let total_lines = doc.rows() as u64;
    let (_, index_bytes) = doc.ram_bytes();
    let g4_10_index = index_bytes * 20; // 20 文件外推
    let g4_10 = g4_10_index <= (256 << 20) * 3 / 10;
    println!(
        "G4-10 索引内存 ≤30%   : {}（单文件 {} KiB × 20 = {} KiB ≤ 76800 KiB；全量索引 {:?}，{} 行）",
        pass(g4_10),
        index_bytes / 1024,
        g4_10_index / 1024,
        index_done,
        total_lines
    );

    // ---- G4-3：跳转 ≤16 ms（1000 次随机跳，含冷缓存） ----
    let mut worst = std::time::Duration::ZERO;
    let mut sum = std::time::Duration::ZERO;
    let jumps = 1000;
    for k in 0..jumps {
        let target = (total_lines as usize * 7919 * (k + 1)) % total_lines as usize;
        doc.evict_decode(); // 冷缓存路径（最坏情况）
        let t = Instant::now();
        let r = doc.row(target).expect("jump row");
        sum += t.elapsed();
        worst = worst.max(t.elapsed());
        let _ = tiderm_textdoc::contract::row_bytes(r).len();
    }
    let avg = sum / jumps as u32;
    let g4_3 = worst.as_millis() <= 16;
    println!(
        "G4-3 跳转 ≤16 ms      : {}（{} 次随机冷跳 worst {:?} avg {:?}）",
        pass(g4_3),
        jumps,
        worst,
        avg
    );

    // ---- G4-7：冷层压缩比 ≥3×（真实结构语料，64 KiB 块，zstd-1） ----
    let raw = std::fs::read(corpus).expect("read corpus for ratio"); // 语料文件本身（bench 机内存允许 ≤1 GiB… 改用流式分块）
    drop(raw);
    let mut raw_total: u64 = 0;
    let mut comp_total: u64 = 0;
    let mut f = std::fs::File::open(corpus).expect("reopen corpus");
    let mut block = vec![0u8; COLD_BLOCK];
    let blocks = 256; // 16 MiB 采样足够代表
    for _ in 0..blocks {
        let n = std::io::Read::read(&mut f, &mut block).unwrap_or(0);
        if n == 0 {
            break;
        }
        let c = compress_cold(&block[..n]).expect("zstd");
        raw_total += n as u64;
        comp_total += c.len() as u64;
    }
    let ratio = raw_total as f64 / comp_total as f64;
    let g4_7 = ratio >= 3.0;
    println!(
        "G4-7 冷层压缩比 ≥3×   : {}（{:.2}×，{} KiB → {} KiB，zstd-1 / 64 KiB 块）",
        pass(g4_7),
        ratio,
        raw_total / 1024,
        comp_total / 1024
    );

    // ---- G4-5/G4-6：20 文件并发 + 内存不随文件大小线性增长 ----
    let mut mgr = DocumentManager::new(256 << 20);
    mgr.pool().set_terminal_panes(1);
    let t = Instant::now();
    let mut ids_1g = Vec::new();
    for k in 0..20 {
        let id = mgr.open(corpus).expect("open doc");
        ids_1g.push(id);
        let _ = k;
    }
    // 后台索引推进（每文档全量）。
    loop {
        let mut any = false;
        for &id in &ids_1g {
            if let Some(d) = mgr.get_mut(id) {
                any |= d.index_more(16 << 20) > 0;
            }
        }
        if !any {
            break;
        }
    }
    // 触发部分解码缓存（每文档读 10 行）。
    for &id in &ids_1g {
        for i in 0..10 {
            let _ = mgr.row(id, i);
        }
    }
    let usage_1g = mgr.usage();
    let open_20 = t.elapsed();
    let g4_5 = usage_1g.total() <= 256 << 20;
    println!(
        "G4-5 20 文件 ≤256 MiB : {}（记账 {} KiB；20×1 GiB 打开+全量索引+抽样读 {:?}）",
        pass(g4_5),
        usage_1g.total() / 1024,
        open_20
    );

    // G4-6：100 MiB 同结构文档对比（缓存同容量，索引自适应采样收敛到同上限）。
    let small_path = corpus.with_extension("small.log");
    gen_corpus(&small_path, 100 << 20);
    let _guard = SmallFile {
        path: small_path.clone(),
    };
    let mut mgr_s = DocumentManager::new(256 << 20);
    let id_s = mgr_s.open(&small_path).expect("open small");
    loop {
        if let Some(d) = mgr_s.get_mut(id_s)
            && d.index_more(64 << 20) == 0
        {
            break;
        }
    }
    for i in 0..10 {
        let _ = mgr_s.row(id_s, i);
    }
    let usage_s = mgr_s.usage();
    let per_doc_1g = usage_1g.total() / 20;
    let diff_pct = (per_doc_1g.abs_diff(usage_s.total()) * 100)
        .checked_div(per_doc_1g)
        .unwrap_or(0);
    let g4_6 = diff_pct <= 20;
    println!(
        "G4-6 内存不随文件大小 : {}（1 GiB 档 {} B/文档 vs 100 MiB 档 {} B/文档，差 {}%）",
        pass(g4_6),
        per_doc_1g,
        usage_s.total(),
        diff_pct
    );

    println!("=== 判据性质：G4-2/3/7/10 为估算值的实测校准；规模见 ADR-013 ===");
}

fn pass(ok: bool) -> &'static str {
    if ok {
        "✅ PASS"
    } else {
        "❌ FAIL"
    }
}
