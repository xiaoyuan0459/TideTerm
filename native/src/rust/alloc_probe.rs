//! 大块分配取证（`docs/memory-optimization-roadmap.md` §8.6.3 的“临时计数分配器”）。
//!
//! `TIDETERM_TRACE_BIG_ALLOC=<字节数>` 打开：经 Rust `System` 分配器的单次分配 ≥ 阈值
//! （默认 1 MiB）时打印尺寸 + 调用栈，用于把 §8.4 里尺寸公式对不上号的堆块（2 MiB
//! 全零块、1.25 MB 块）归因到具体分配点。默认关闭且走 `AtomicBool` 惰性读，关闭时
//! 对热路径只多一次 relaxed load；这是取证工具，不是常驻功能，别在探针外引用。
//!
//! 重入铁律：打印路径（backtrace 捕获 / stderr 格式化）自己会分配，必须用线程本地
//! 标志挡住重入 —— 第一版没挡，`Backtrace::force_capture` 递归进分配器直接把主线程
//! 栈打爆（stack overflow, exit 127）。

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);
static THRESHOLD: AtomicUsize = AtomicUsize::new(0);
static SEEN: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static IN_TRACE: Cell<bool> = const { Cell::new(false) };
}

fn probe_on() -> bool {
    // 惰性初始化：第一次分配时读一次环境变量，之后纯原子读。读 env 本身会分配，
    // 会递归回 alloc —— 所以先立 usize::MAX 哨兵（任何真实 size 都小于它，重入直接
    // 跳过），读完再落真值；不这样做就是无界递归 → 主线程栈溢出。
    if ENABLED.load(Ordering::Relaxed) {
        return true;
    }
    if THRESHOLD.load(Ordering::Relaxed) != 0 {
        return false; // 已初始化但未开启（或正在初始化的哨兵）。
    }
    THRESHOLD.store(usize::MAX, Ordering::Relaxed);
    let threshold = std::env::var("TIDETERM_TRACE_BIG_ALLOC")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    THRESHOLD.store(threshold.max(1), Ordering::Relaxed);
    let on = threshold > 0;
    ENABLED.store(on, Ordering::Relaxed);
    on
}

fn trace(kind: &str, size: usize) {
    IN_TRACE.with(|in_trace| {
        if in_trace.get() {
            return; // 打印路径自身的分配：不追、直接放行。
        }
        in_trace.set(true);
        let n = SEEN.fetch_add(1, Ordering::Relaxed);
        eprintln!(
            "[bigalloc #{n}] {kind} {size} B\n{}",
            Backtrace::force_capture()
        );
        in_trace.set(false);
    });
}

pub struct BigAllocProbe;

unsafe impl GlobalAlloc for BigAllocProbe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forward the caller's valid layout unchanged to System.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() && layout.size() >= THRESHOLD.load(Ordering::Relaxed) && probe_on() {
            trace("alloc", layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forward the caller's valid layout unchanged to System.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() && layout.size() >= THRESHOLD.load(Ordering::Relaxed) && probe_on() {
            trace("alloc_zeroed", layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: ptr and layout are the unchanged allocation supplied by the caller.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forward the caller's allocation and valid new size unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}
