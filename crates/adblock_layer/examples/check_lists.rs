//! Downloads the real filter lists (cached), builds the blocker and checks a
//! few known ad / tracker / normal URLs.
//!
//! `cargo run -p adblock_layer --example check_lists [cache_dir]`

use std::time::{Duration, Instant};

use adblock_layer::{Blocker, DEFAULT_LISTS, ListCache, ResourceKind};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("glowtune-filters"));
    let cache = ListCache::new(&dir).expect("cache dir");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let report = rt.block_on(cache.refresh(DEFAULT_LISTS, Duration::from_secs(7 * 24 * 3600)));
    println!(
        "lists updated {:?} failed {:?} skipped {:?}",
        report.updated, report.failed, report.skipped
    );

    let lists = cache.load_cached(DEFAULT_LISTS);
    let bytes: usize = lists.iter().map(|l| l.len()).sum();
    let started = Instant::now();
    let blocker = Blocker::from_lists(&lists);
    println!(
        "{} lists, {:.1} MB of text → {} rules in {:?}",
        lists.len(),
        bytes as f64 / 1e6,
        blocker.rule_count(),
        started.elapsed()
    );

    let yt = "https://music.youtube.com/";
    let sc = "https://soundcloud.com/discover";
    let cases = [
        // (expect_block, url, page, kind)
        (
            true,
            "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
            yt,
            ResourceKind::Script,
        ),
        (
            true,
            "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
            yt,
            ResourceKind::Script,
        ),
        (
            true,
            "https://www.google-analytics.com/analytics.js",
            sc,
            ResourceKind::Script,
        ),
        (
            true,
            "https://static.doubleclick.net/instream/ad_status.js",
            yt,
            ResourceKind::Script,
        ),
        (
            false,
            "https://music.youtube.com/youtubei/v1/browse",
            yt,
            ResourceKind::Fetch,
        ),
        (
            false,
            "https://rr3---sn-abc.googlevideo.com/videoplayback?mime=audio",
            yt,
            ResourceKind::Media,
        ),
        (
            false,
            "https://api-v2.soundcloud.com/stream",
            sc,
            ResourceKind::XmlHttpRequest,
        ),
        (
            false,
            "https://a-v2.sndcdn.com/assets/app.js",
            sc,
            ResourceKind::Script,
        ),
    ];
    let mut ok = 0;
    for (expect, url, page, kind) in cases {
        let got = blocker.check(url, page, kind).block;
        let mark = if got == expect {
            ok += 1;
            "ok "
        } else {
            "BAD"
        };
        println!("{mark} {:5} {url}", if got { "block" } else { "allow" });
    }
    println!("{ok}/{} as expected", cases.len());
    println!(
        "cosmetic CSS for YouTube Music: {} bytes",
        blocker.cosmetic_css(yt).len()
    );
    println!(
        "cosmetic CSS for SoundCloud:    {} bytes",
        blocker.cosmetic_css(sc).len()
    );
    println!(
        "site-only CSS: YouTube Music {} bytes, SoundCloud {} bytes",
        blocker.site_css(yt).len(),
        blocker.site_css(sc).len()
    );

    let started = Instant::now();
    for _ in 0..10_000 {
        blocker.check(
            "https://music.youtube.com/youtubei/v1/next",
            yt,
            ResourceKind::Fetch,
        );
    }
    println!("check(): {:?} per request", started.elapsed() / 10_000);
}
