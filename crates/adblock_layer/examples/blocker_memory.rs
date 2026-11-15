//! How much heap the blocker holds once built from the cached filter lists.
//!
//! `cargo run --release -p adblock_layer --example blocker_memory [cache_dir]`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use adblock_layer::{Blocker, DEFAULT_LISTS, ListCache};

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let now =
            LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed) + layout.size() as isize;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn mb(bytes: isize) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("glowtune-filters"));
    let cache = ListCache::new(&dir).expect("cache dir");
    let lists = cache.load_cached(DEFAULT_LISTS);
    let text = LIVE.load(Ordering::Relaxed);
    PEAK.store(text, Ordering::Relaxed);
    let blocker = Blocker::from_lists(&lists);
    drop(lists);
    let after = LIVE.load(Ordering::Relaxed);
    println!(
        "{} rules: list text {:.1} MB; blocker holds {:.1} MB; peak while building {:.1} MB",
        blocker.rule_count(),
        mb(text),
        mb(after),
        mb(PEAK.load(Ordering::Relaxed))
    );
}
