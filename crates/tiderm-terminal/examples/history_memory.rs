//! Release comparison: one warmup, three samples, real retained heap and full feed work.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
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
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if size >= layout.size() {
            LIVE.fetch_add(size - layout.size(), Ordering::Relaxed);
        } else {
            LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("use --release");
        std::process::exit(2);
    }
    let kind = std::env::args().nth(1).unwrap_or_else(|| "short".into());
    let mut input = String::new();
    for row in 0..4000 {
        let text = match kind.as_str() {
            "short" => format!("line {row:05} 中文 abc"),
            "prefix" => format!("{}{row:05} payload 中文 abc", "P".repeat(80)),
            "unique" => {
                let body: String = (0..140)
                    .map(|col| char::from(b'a' + ((row * 7 + col * 11) % 26) as u8))
                    .collect();
                format!("{row:05} {body}")
            }
            _ => panic!("short, prefix or unique"),
        };
        input.push_str(&text);
        input.push_str("\r\n");
    }
    for sample in 0..4 {
        let before = LIVE.load(Ordering::Relaxed);
        let mut grid = TermGrid::with_history(200, 30, 64, 32 << 20);
        let start = Instant::now();
        for chunk in input.as_bytes().chunks(16384) {
            grid.feed(chunk);
        }
        let retained = grid.total_history();
        let bytes = grid.scrollback_bytes();
        let seconds = start.elapsed().as_secs_f64();
        let heap = LIVE.load(Ordering::Relaxed).saturating_sub(before);
        let first = grid.line_text(-(retained as i32)).unwrap();
        assert!(
            first.contains("00000"),
            "history cannot be discarded to improve the measurement"
        );
        assert!(retained >= 3970 && bytes <= 32 << 20);
        println!("kind={kind} sample={sample} warmup={} seconds={seconds:.6} heap_bytes={heap} store_bytes={bytes} retained={retained}", sample == 0);
    }
}
