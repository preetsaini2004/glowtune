//! Ad blocking for the service webviews (PLAN.md section 6, web part).
//!
//! ```text
//!  startup ─► cached filter lists ─► Blocker (background thread)
//!          └► weekly refresh (EasyList, EasyPrivacy, uBO) ─► rebuild Blocker
//!
//!  service webview ── every request ──► WebResourceRequested ─► Blocker::check
//!                                                         blocked → empty 403
//!                  ── every page ─────► injected <style> hiding ad elements
//! ```
//! Personal use. YouTube fights ad blockers, so some ads may still slip through
//! until the lists catch up (filters update weekly).

use std::sync::{Arc, RwLock};
use std::time::Duration;

use adblock_layer::{Blocker, DEFAULT_LISTS, ListCache};

const MAX_LIST_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// The current blocker, swapped atomically when lists update.
#[derive(Clone)]
pub struct AdBlock {
    blocker: Arc<RwLock<Arc<Blocker>>>,
}

impl AdBlock {
    /// Loads cached lists in the background, then refreshes stale ones.
    pub fn start(cache_dir: std::path::PathBuf) -> Self {
        let this = Self {
            blocker: Arc::new(RwLock::new(Arc::new(Blocker::empty()))),
        };
        let handle = this.clone();
        std::thread::Builder::new()
            .name("glowtune-adblock".into())
            .spawn(move || {
                let cache = match ListCache::new(cache_dir) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("ad block off: {e}");
                        return;
                    }
                };
                handle.rebuild(&cache);
                // Refresh lists older than a week, then rebuild if anything changed.
                let report =
                    tauri::async_runtime::block_on(cache.refresh(DEFAULT_LISTS, MAX_LIST_AGE));
                for (id, err) in &report.failed {
                    tracing::warn!("filter list {id} not updated: {err}");
                }
                if !report.updated.is_empty() {
                    tracing::info!("filter lists updated: {:?}", report.updated);
                    handle.rebuild(&cache);
                }
            })
            .expect("spawn ad block thread");
        this
    }

    fn rebuild(&self, cache: &ListCache) {
        let lists = cache.load_cached(DEFAULT_LISTS);
        if lists.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        let blocker = Blocker::from_lists(&lists);
        tracing::info!(
            "ad block ready: {} rules in {:?}",
            blocker.rule_count(),
            started.elapsed()
        );
        if let Ok(mut slot) = self.blocker.write() {
            *slot = Arc::new(blocker);
        }
    }

    pub fn current(&self) -> Arc<Blocker> {
        self.blocker
            .read()
            .map(|b| b.clone())
            .unwrap_or_else(|_| Arc::new(Blocker::empty()))
    }

    pub fn blocked_count(&self) -> u64 {
        self.current().blocked_count()
    }
}

/// Ad-slot stripping + skip backup for YouTube Music (see the file's header).
#[cfg(windows)]
const YOUTUBE_ADS: &str = include_str!("youtube_ads.js");

/// Hooks a service webview: network blocking + cosmetic CSS.
#[cfg(windows)]
pub fn install(
    window: &tauri::WebviewWindow,
    adblock: AdBlock,
    home_url: &'static str,
) -> tauri::Result<()> {
    use adblock_layer::ResourceKind;
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
    use windows::core::{HSTRING, PWSTR, w};

    fn kind(ctx: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> ResourceKind {
        match ctx {
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => ResourceKind::Document,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => ResourceKind::Stylesheet,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => ResourceKind::Script,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => ResourceKind::Image,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => ResourceKind::Font,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA => ResourceKind::Media,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST => ResourceKind::XmlHttpRequest,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => ResourceKind::Fetch,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => ResourceKind::WebSocket,
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => ResourceKind::Ping,
            _ => ResourceKind::Other,
        }
    }

    // Element hiding for the service's pages, injected before page scripts run.
    let css = adblock.current().site_css(home_url);
    // Only on the service's own pages (not on Google / SoundCloud sign-in pages).
    let host = home_url
        .split("//")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("")
        .to_string();
    let host_json = serde_json::to_string(&host).unwrap_or_else(|_| "\"\"".into());
    let css_json = serde_json::to_string(&css).unwrap_or_else(|_| "\"\"".into());
    let script = format!(
        "(() => {{ const css = {css_json}; if (!css || location.hostname !== {host_json}) return; const add = () => {{ const s = document.createElement('style'); \
         s.id = 'glowtune-adblock'; s.textContent = css; (document.head || document.documentElement).appendChild(s); }}; \
         if (document.documentElement) add(); else document.addEventListener('DOMContentLoaded', add); }})();"
    );

    window.with_webview(move |webview| unsafe {
        let result = (|| -> windows::core::Result<()> {
            let core = webview.controller().CoreWebView2()?;
            let env = webview.environment();
            core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), None)?;
            // YouTube Music's video ads share servers with the music: page-level fix.
            if host == "music.youtube.com" {
                core.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(YOUTUBE_ADS), None)?;
            }
            core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
            let mut token = 0i64;
            core.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                    let (Some(sender), Some(args)) = (sender, args) else {
                        return Ok(());
                    };
                    let request = args.Request()?;
                    let mut uri = PWSTR::null();
                    request.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    let mut source = PWSTR::null();
                    sender.Source(&mut source)?;
                    let source = take_pwstr(source);
                    let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
                    args.ResourceContext(&mut ctx)?;

                    if adblock.current().check(&uri, &source, kind(ctx)).block {
                        let response =
                            env.CreateWebResourceResponse(None, 403, w!("Blocked"), w!(""))?;
                        args.SetResponse(&response)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            tracing::error!("ad block hook failed: {e}");
        }
    })
}

#[cfg(not(windows))]
pub fn install(_: &tauri::WebviewWindow, _: AdBlock, _: &'static str) -> tauri::Result<()> {
    Ok(()) // WebKitGTK content filters come with the Linux port.
}
