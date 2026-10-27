//! 引擎常驻足迹的**精确**测量（计数全局分配器）。
//!
//! 进程级 RSS 只能看到"整窗花了多少"，归因靠猜；这里把 `TermGrid` 构造 + 喂入后的
//! **存活堆字节**直接量出来，用来判断"每窗格那 1–2 MB 到底是谁"。
//!
//! 单测文件只放一个测试：并发分配会污染计数，跑法固定
//! `cargo test --test engine_footprint -- --nocapture --test-threads=1`。

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use tiderm_terminal::TermGrid;

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size >= layout.size() {
            LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

fn kb(bytes: usize) -> f64 {
    bytes as f64 / 1024.0
}

/// 200 列（最大化窗口的常见宽度）与 96 列（四分屏）两档，喂 1 行与喂 4000 行各测一次。
#[test]
fn report_engine_footprint() {
    for cols in [96usize, 200] {
        for live_window in [64usize, 200] {
            for lines in [1usize, 4000] {
                let base = live();
                let mut g = TermGrid::with_history(cols, 30, live_window, 32 << 20);
                let created = live() - base;
                let mut payload = String::with_capacity(lines * 24);
                for i in 0..lines {
                    payload.push_str(&format!("line {i:05} 中文 abc\r\n"));
                }
                g.feed(payload.as_bytes());
                // Synchronize completed history before sampling the allocator.
                let retained = g.total_history();
                let stored_bytes = g.scrollback_bytes();
                let fed = live() - base;
                println!(
                    "[footprint] {cols} 列 × 30 行, live_window={live_window}, 喂 {lines} 行 → \
                     构造 {:.0} KB, 喂后 {:.0} KB (历史 {} 行, 回滚 {:.0} KB, 引擎 {:.0} KB)",
                    kb(created),
                    kb(fed),
                    retained,
                    kb(stored_bytes),
                    kb(fed.saturating_sub(stored_bytes)),
                );
                // 回归闸门：200 列的原始网格一旦回到"首次滚动就预分配 1000 行"
                // （vendor/alacritty_terminal 的补丁被上游覆盖），引擎会从 1.4–2.6 MB
                // 涨到 ~5.5 MB。
                if cols == 200 && lines > 1 {
                    let engine = fed.saturating_sub(stored_bytes);
                    assert!(
                        engine < 3_500 * 1024,
                        "200 列原始网格 {:.0} KB，超过 3.5 MB —— 检查 \
                         vendor/alacritty_terminal/src/grid/storage.rs 的 cache_limit 补丁",
                        kb(engine)
                    );
                }
                drop(g);
            }
        }
    }
}
