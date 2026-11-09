//! Service views (PLAN.md section 5): Spotify, Apple Music, YouTube Music and
//! SoundCloud web players in their own window, logged in once inside the app
//! (each service keeps its own saved profile).
//!
//! ```text
//!  tray / player ──open──► one "service" window (the other one is destroyed first)
//!                             │  own WebView2 profile per service → logins survive restarts
//!                             └─ plays audio → Windows media controls → hub (glow, lyrics)
//! ```
//!
//! Security: the service window loads remote sites, so it's in no Tauri
//! capability (see capabilities/default.json): pages can't call app commands.

use std::sync::Mutex;

use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SERVICE: &str = "service";

/// Tauri's default WebView2 flags, plus autoplay so "play" from Glowtune can
/// start music without a click inside the page.
pub(crate) const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \n     --autoplay-policy=no-user-gesture-required";

/// A "press play on the top result" script waiting for its page to finish loading.
static PENDING_PLAY: Mutex<Option<(Service, bool, String)>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Service {
    Spotify,
    AppleMusic,
    YoutubeMusic,
    Soundcloud,
}

impl Service {
    pub fn id(self) -> &'static str {
        match self {
            Service::Spotify => "spotify",
            Service::AppleMusic => "apple-music",
            Service::YoutubeMusic => "youtube-music",
            Service::Soundcloud => "soundcloud",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Service::Spotify => "Spotify",
            Service::AppleMusic => "Apple Music",
            Service::YoutubeMusic => "YouTube Music",
            Service::Soundcloud => "SoundCloud",
        }
    }

    pub(crate) fn url(self) -> &'static str {
        match self {
            Service::Spotify => "https://open.spotify.com/",
            Service::AppleMusic => "https://music.apple.com/",
            Service::YoutubeMusic => "https://music.youtube.com/",
            Service::Soundcloud => "https://soundcloud.com/discover",
        }
    }

    /// The service's own search page for `query`.
    pub fn search_url(self, query: &str) -> String {
        let q = crate::search::encode(query);
        match self {
            Service::Spotify => format!("https://open.spotify.com/search/{q}"),
            Service::AppleMusic => format!("https://music.apple.com/search?term={q}"),
            Service::YoutubeMusic => format!("https://music.youtube.com/search?q={q}"),
            Service::Soundcloud => format!("https://soundcloud.com/search?q={q}"),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "spotify" => Some(Service::Spotify),
            "apple-music" => Some(Service::AppleMusic),
            "youtube-music" => Some(Service::YoutubeMusic),
            "soundcloud" => Some(Service::Soundcloud),
            _ => None,
        }
    }
}

/// Which service the open window shows (window titles change while browsing).
pub(crate) fn open_service(app: &AppHandle) -> Option<Service> {
    let w = app.get_webview_window(SERVICE)?;
    let url = w.url().ok()?;
    let host = url.host_str()?;
    if host.ends_with("spotify.com") {
        Some(Service::Spotify)
    } else if host.ends_with("apple.com") {
        Some(Service::AppleMusic)
    } else if host.ends_with("youtube.com") || host.ends_with("google.com") {
        Some(Service::YoutubeMusic)
    } else if host.ends_with("soundcloud.com") {
        Some(Service::Soundcloud)
    } else {
        None
    }
}

/// Opens (or focuses) a service. Only one service webview lives at a time:
/// each costs ~100+ MB, so switching destroys the previous one.
pub fn show(app: &AppHandle, service: Service) -> tauri::Result<()> {
    show_at(app, service, None)
}

/// Like [`show`], but lands on `url` (a page of that service, e.g. its search).
pub fn show_at(app: &AppHandle, service: Service, url: Option<String>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(SERVICE) {
        if open_service(app) == Some(service) {
            if let Some(url) = url.as_deref().and_then(|u| u.parse().ok()) {
                w.navigate(url)?;
            }
            w.unminimize()?;
            w.show()?;
            return w.set_focus();
        }
        w.destroy()?;
    }
    match build(app, service, url.as_deref()) {
        Err(e) if is_profile_busy(&e) => {
            // The previous webview's helper processes still hold the profile
            // for a moment after closing. Retry off the main thread.
            let app = app.clone();
            let url = url.clone();
            std::thread::spawn(move || {
                for attempt in 1..=6 {
                    std::thread::sleep(std::time::Duration::from_millis(250 * attempt));
                    match build(&app, service, url.as_deref()) {
                        Ok(()) => return,
                        Err(e) if is_profile_busy(&e) => continue,
                        Err(e) => {
                            tracing::error!("could not open {}: {e}", service.title());
                            return;
                        }
                    }
                }
                tracing::error!(
                    "could not open {}: its profile stayed busy",
                    service.title()
                );
            });
            Ok(())
        }
        other => other,
    }
}

/// WebView2 error 0x800700AA: "The requested resource is in use".
fn is_profile_busy(e: &tauri::Error) -> bool {
    let text = e.to_string();
    text.contains("800700AA") || text.contains("resource is in use")
}

fn build(app: &AppHandle, service: Service, start: Option<&str>) -> tauri::Result<()> {
    let url = start
        .and_then(|u| u.parse().ok())
        .unwrap_or_else(|| service.url().parse().expect("static service URL is valid"));
    let profile = app
        .path()
        .app_local_data_dir()?
        .join("services")
        .join(service.id());
    let window = WebviewWindowBuilder::new(app, SERVICE, WebviewUrl::External(url))
        .title(format!("{} · Glowtune", service.title()))
        .inner_size(1120.0, 760.0)
        .min_inner_size(640.0, 480.0)
        .center()
        // Separate WebView2 profile per service: cookies/logins persist and
        // never mix between services.
        .data_directory(profile)
        .disable_drag_drop_handler()
        .additional_browser_args(BROWSER_ARGS)
        .on_new_window(move |url, _| popup(service, &url))
        .on_page_load(move |w, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let due = PENDING_PLAY.lock().ok().and_then(|mut p| {
                p.take_if(|(s, _, url)| *s == service && payload.url().as_str() == url)
            });
            if let Some((_, direct, _)) = due
                && let Err(e) = w.eval(if direct {
                    direct_play_script(service)
                } else {
                    autoplay_script(service)
                })
            {
                tracing::warn!("autoplay on {}: {e}", service.title());
            }
        })
        .build()?;
    let block_ads = app
        .state::<crate::AppSettings>()
        .current
        .lock()
        .map(|s| s.block_ads)
        .unwrap_or(true);
    if block_ads {
        crate::adblock::install(
            &window,
            app.state::<crate::adblock::AdBlock>().inner().clone(),
            service.url(),
        )?;
    }
    tracing::info!("opened {}", service.title());
    Ok(())
}

/// A page asked for a new window (`window.open`, `target="_blank"` links).
///
/// ```text
///  sign-in popup (Google / Apple / Facebook / the service itself)
///      └─► real popup, same profile → it can hand the login back to the page
///  any other web link ─► the default browser
/// ```
fn popup(service: Service, url: &tauri::Url) -> NewWindowResponse<tauri::Wry> {
    if url.as_str() == "about:blank" || is_sign_in(service, url) {
        return NewWindowResponse::Allow;
    }
    if matches!(url.scheme(), "http" | "https") {
        open_in_browser(url.as_str());
    }
    NewWindowResponse::Deny
}

/// Sites that sign-in popups come from.
fn is_sign_in(service: Service, url: &tauri::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let site = |d: &str| host == d || host.ends_with(&format!(".{d}"));
    const PROVIDERS: [&str; 6] = [
        "accounts.google.com",
        "appleid.apple.com",
        "idmsa.apple.com",
        "facebook.com",
        "login.live.com",
        "login.microsoftonline.com",
    ];
    PROVIDERS.iter().any(|d| site(d))
        || match service {
            Service::Spotify => site("spotify.com"),
            Service::AppleMusic => site("apple.com"),
            Service::YoutubeMusic => site("youtube.com") || site("google.com"),
            Service::Soundcloud => site("soundcloud.com"),
        }
}

/// Opens a web link in the user's browser (at most one a second, so a page
/// can't flood it with tabs).
fn open_in_browser(url: &str) {
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    if let Ok(mut last) = LAST.lock() {
        if last.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::{HSTRING, PCWSTR, w};
        // SAFETY: shell call with strings that outlive it; `url` is http(s) only.
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &HSTRING::from(url),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Finds `query` in `service` and starts the best match: opens the service's
/// search page, then presses play on the top result once the page is ready.
/// Logged in once, it simply plays; logged out, the service asks to sign in.
pub fn play(app: &AppHandle, service: Service, query: &str) -> tauri::Result<()> {
    if let Ok(mut p) = PENDING_PLAY.lock() {
        *p = Some((service, false, service.play_url(query)));
    }
    show_at(app, service, Some(service.play_url(query)))
}

pub fn play_link(app: &AppHandle, service: Service, url: String) -> tauri::Result<()> {
    if let Ok(mut pending) = PENDING_PLAY.lock() {
        *pending = Some((service, true, url.clone()));
    }
    show_at(app, service, Some(url))
}

fn direct_play_script(service: Service) -> String {
    let selector = match service {
        Service::Spotify => "main [data-testid=play-button]",
        Service::YoutubeMusic => "ytmusic-player-bar [aria-label=Play]",
        Service::AppleMusic => "main button[data-testid=play-button], main button.play-button",
        Service::Soundcloud => ".listenEngagement__playButton .playButton:not(.playing)",
    };
    format!(
        r#"(() => {{ let tries=0; const timer=setInterval(() => {{ const button=document.querySelector('{selector}'); if(button){{if(!/pause/i.test(button.getAttribute('aria-label')||''))button.click();clearInterval(timer);}} else if(document.querySelector('ytmusic-player-bar [aria-label=Pause]')||++tries>=40)clearInterval(timer); }},500); }})()"#
    )
}

impl Service {
    /// Search page that lists playable *songs* first.
    fn play_url(self, query: &str) -> String {
        let q = crate::search::encode(query);
        match self {
            Service::Spotify => format!("https://open.spotify.com/search/{q}/tracks"),
            Service::Soundcloud => format!("https://soundcloud.com/search/sounds?q={q}"),
            _ => self.search_url(query),
        }
    }
}

/// Page script: waits (up to 20 s) for the first result, then plays it.
/// Selectors follow each site's current markup; if a site changes, the user
/// still lands on the right search page and can click play themselves.
fn autoplay_script(service: Service) -> String {
    let body = match service {
        Service::Spotify => {
            r#"const row = document.querySelector('[data-testid="tracklist-row"]');
               if (!row) return false;
               const btn = row.querySelector('button[data-testid="play-button"], button[aria-label^="Play"]');
               if (btn) btn.click();
               else row.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
               return true;"#
        }
        Service::YoutubeMusic => {
            r#"const btn = document.querySelector(
                 'ytmusic-card-shelf-renderer ytmusic-play-button-renderer,' +
                 'ytmusic-shelf-renderer ytmusic-responsive-list-item-renderer ytmusic-play-button-renderer');
               if (!btn) return false;
               btn.click();
               return true;"#
        }
        Service::Soundcloud => {
            r#"const btn = document.querySelector('.searchList__item .sc-button-play, .searchList__item .playButton');
               if (!btn) return false;
               btn.click();
               return true;"#
        }
        Service::AppleMusic => {
            r#"const btn = document.querySelector(
                 '.top-search-lockup [data-testid="play-button"], [data-testid="play-button"], button.play-button');
               if (!btn) return false;
               btn.click();
               return true;"#
        }
    };
    format!(
        r#"(() => {{
  if (window.__glowtunePlay) return;
  window.__glowtunePlay = true;
  const started = Date.now();
  const attempt = () => {{ {body} }};
  const timer = setInterval(() => {{
    let done = false;
    try {{ done = attempt(); }} catch (e) {{}}
    if (done || Date.now() - started > 20000) {{
      clearInterval(timer);
      window.__glowtunePlay = false;
    }}
  }}, 400);
}})();"#
    )
}

/// Shuffle / repeat / like / volume as the service's own web player shows them.
/// Browsers don't pass these to Windows media controls, so for music playing in
/// a service window Glowtune reads and presses the page's own buttons.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPlayerState {
    pub shuffle: Option<bool>,
    /// "off" | "list" | "track"
    pub repeat: Option<String>,
    pub liked: Option<bool>,
    /// 0..=1
    pub volume: Option<f64>,
}

/// `action`: "state" (just read), "shuffle", "repeat", "like", "volume" (with `value` 0..1).
/// Returns the state after the action, or `None` if no service window is open
/// or the page doesn't have that player.
pub async fn web_player(
    app: &AppHandle,
    action: &str,
    value: Option<f64>,
) -> Option<WebPlayerState> {
    let window = app.get_webview_window(SERVICE)?;
    let service = open_service(app)?;
    let action = match action {
        "state" | "shuffle" | "repeat" | "like" | "volume" => action,
        _ => return None,
    };
    let value = value
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let js = web_player_script(service, action, value)?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    run_script(&window, js, tx).ok()?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_millis(1500)).ok()
    })
    .await
    .ok()
    .flatten()?;
    serde_json::from_str::<Option<WebPlayerState>>(&out)
        .ok()
        .flatten()
}

#[cfg(windows)]
pub(crate) fn run_script(
    window: &tauri::WebviewWindow,
    js: String,
    tx: std::sync::mpsc::Sender<String>,
) -> tauri::Result<()> {
    use webview2_com::ExecuteScriptCompletedHandler;
    use windows::core::HSTRING;
    window.with_webview(move |webview| {
        // SAFETY: COM calls on the webview's own (main) thread, inside with_webview.
        let result = unsafe {
            webview.controller().CoreWebView2().and_then(|core| {
                core.ExecuteScript(
                    &HSTRING::from(js.as_str()),
                    &ExecuteScriptCompletedHandler::create(Box::new(move |error, result| {
                        let _ = tx.send(if error.is_ok() { result } else { "null".into() });
                        Ok(())
                    })),
                )
            })
        };
        if let Err(e) = result {
            tracing::warn!("web player script: {e}");
        }
    })
}

#[cfg(not(windows))]
pub(crate) fn run_script(
    _window: &tauri::WebviewWindow,
    _js: String,
    _tx: std::sync::mpsc::Sender<String>,
) -> tauri::Result<()> {
    Ok(())
}

/// Page script for one service: optionally press a control, then report state.
/// Selectors follow each site's current markup and fail soft (null fields).
fn web_player_script(service: Service, action: &str, value: f64) -> Option<String> {
    let body = match service {
        Service::Spotify => {
            r##"const $ = (s) => document.querySelector(s);
  const sh = $('[data-testid="control-button-shuffle"]');
  const rp = $('[data-testid="control-button-repeat"]');
  const lk = $('[data-testid="now-playing-widget"] [data-testid="add-button"], [data-testid="now-playing-widget"] button[aria-checked]');
  const vol = $('[data-testid="volume-bar"] input[type="range"]');
  if (act === "shuffle" && sh) sh.click();
  if (act === "repeat" && rp) rp.click();
  if (act === "like" && lk) lk.click();
  if (act === "volume" && vol) setRange(vol, Number(vol.min || 0) + val * (Number(vol.max || 1) - Number(vol.min || 0)));
  const checked = (b) => (b ? b.getAttribute("aria-checked") === "true" : null);
  let repeat = null;
  if (rp) {
    const label = (rp.getAttribute("aria-label") || "").toLowerCase();
    repeat = label.includes("disable") ? "track" : checked(rp) ? "list" : "off";
  }
  const v = vol ? (Number(vol.value) - Number(vol.min || 0)) / ((Number(vol.max || 1) - Number(vol.min || 0)) || 1) : null;
  return { shuffle: checked(sh), repeat, liked: checked(lk), volume: v };"##
        }
        Service::YoutubeMusic => {
            r##"const bar = document.querySelector("ytmusic-player-bar");
  if (!bar) return null;
  const sh = bar.querySelector(".shuffle");
  const rp = bar.querySelector(".repeat");
  const lk = bar.querySelector("ytmusic-like-button-renderer #button-shape-like button, ytmusic-like-button-renderer .like");
  const video = document.querySelector("video");
  if (act === "shuffle" && sh) sh.click();
  if (act === "repeat" && rp) rp.click();
  if (act === "like" && lk) lk.click();
  if (act === "volume" && video) { video.volume = val; const s = bar.querySelector("#volume-slider"); if (s) s.value = Math.round(val * 100); }
  const label = rp ? (rp.getAttribute("title") || rp.getAttribute("aria-label") || "").toLowerCase() : "";
  const repeat = rp ? (label.includes("one") ? "track" : label.includes("all") ? "list" : "off") : null;
  const liked = lk ? (lk.getAttribute("aria-pressed") === "true") : null;
  return { shuffle: sh ? false : null, repeat, liked, volume: video ? video.volume : null };"##
        }
        Service::Soundcloud => {
            r##"const $ = (s) => document.querySelector(s);
  const sh = $(".playControls .shuffleControl");
  const rp = $(".playControls .repeatControl");
  const lk = $(".playControls .playbackSoundBadge .sc-button-like");
  if (act === "shuffle" && sh) sh.click();
  if (act === "repeat" && rp) rp.click();
  if (act === "like" && lk) lk.click();
  // The audio element isn't in the page: drive SoundCloud's own (vertical)
  // volume slider with a click at the right height, so its UI stays in step.
  const vs = $(".playControls .volume__sliderWrapper");
  const bg = $(".playControls .volume__sliderBackground");
  if (act === "volume" && vs && bg) {
    const r = bg.getBoundingClientRect();
    const press = (y) => {
      const o = { bubbles: true, cancelable: true, view: window, button: 0, buttons: 1,
        clientX: r.left + r.width / 2, clientY: y };
      vs.dispatchEvent(new MouseEvent("mousedown", o));
      document.dispatchEvent(new MouseEvent("mousemove", o));
      document.dispatchEvent(new MouseEvent("mouseup", o));
    };
    if (r.height > 0) {
      // The ends are pressed past the bar so they land exactly on 0 and 1.
      let y = val <= 0.01 ? r.bottom + 20 : val >= 0.99 ? r.top - 20 : r.bottom - val * r.height;
      press(y);
      // The handle's size shifts the scale a little: correct once from the result.
      const got = parseFloat(vs.getAttribute("aria-valuenow"));
      if (val > 0.01 && val < 0.99 && Number.isFinite(got) && Math.abs(got - val) > 0.01) {
        press(y + (got - val) * r.height);
      }
    }
  }
  const repeat = rp ? (rp.classList.contains("m-one") ? "track" : rp.classList.contains("m-all") ? "list" : "off") : null;
  const level = vs ? parseFloat(vs.getAttribute("aria-valuenow")) : NaN;
  return {
    shuffle: sh ? sh.classList.contains("m-shuffling") : null,
    repeat,
    liked: lk ? lk.classList.contains("sc-button-selected") : null,
    volume: Number.isFinite(level) ? Math.min(1, Math.max(0, level)) : null,
  };"##
        }
        Service::AppleMusic => return None,
    };
    Some(format!(
        r#"(() => {{
  const act = "{action}";
  const val = {value};
  const setRange = (el, v) => {{
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    set.call(el, String(v));
    el.dispatchEvent(new Event("input", {{ bubbles: true }}));
    el.dispatchEvent(new Event("change", {{ bubbles: true }}));
  }};
  try {{
    {body}
  }} catch (e) {{
    return null;
  }}
}})()"#
    ))
}

/// Play/pause, next and previous pressed on the service page's own buttons.
/// Windows media controls don't reliably reach a web player inside a WebView,
/// so for music playing in a service window this is the dependable path.
/// Returns true if the page had the button.
pub async fn web_transport(app: &AppHandle, action: &str) -> bool {
    let (Some(window), Some(service)) = (app.get_webview_window(SERVICE), open_service(app)) else {
        return false;
    };
    let selector = match (service, action) {
        (Service::Spotify, "playPause") => r#"[data-testid="control-button-playpause"]"#,
        (Service::Spotify, "next") => r#"[data-testid="control-button-skip-forward"]"#,
        (Service::Spotify, "previous") => r#"[data-testid="control-button-skip-back"]"#,
        (Service::YoutubeMusic, "playPause") => "ytmusic-player-bar #play-pause-button",
        (Service::YoutubeMusic, "next") => "ytmusic-player-bar .next-button",
        (Service::YoutubeMusic, "previous") => "ytmusic-player-bar .previous-button",
        (Service::Soundcloud, "playPause") => ".playControls__play",
        (Service::Soundcloud, "next") => ".playControls__next",
        (Service::Soundcloud, "previous") => ".playControls__prev",
        _ => return false,
    };
    let js = format!(
        "(() => {{ const b = document.querySelector({}); if (!b) return false; b.click(); return true; }})()",
        serde_json::to_string(selector).unwrap_or_default()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    if run_script(&window, js, tx).is_err() {
        return false;
    }
    tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(std::time::Duration::from_millis(1500)).ok()
    })
    .await
    .ok()
    .flatten()
    .is_some_and(|r| r == "true")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign_in(service: Service, url: &str) -> bool {
        is_sign_in(service, &url.parse().unwrap())
    }

    #[test]
    fn sign_in_popups_stay_in_app() {
        assert!(sign_in(
            Service::Soundcloud,
            "https://accounts.google.com/o/oauth2/auth?x=1"
        ));
        assert!(sign_in(
            Service::Soundcloud,
            "https://www.facebook.com/v12.0/dialog/oauth"
        ));
        assert!(sign_in(
            Service::Soundcloud,
            "https://appleid.apple.com/auth/authorize"
        ));
        assert!(sign_in(
            Service::Soundcloud,
            "https://secure.soundcloud.com/connect"
        ));
        assert!(sign_in(
            Service::Spotify,
            "https://accounts.spotify.com/login"
        ));
    }

    #[test]
    fn other_links_go_to_the_browser() {
        assert!(!sign_in(
            Service::Soundcloud,
            "https://apps.apple.com/app/soundcloud"
        ));
        assert!(!sign_in(
            Service::Soundcloud,
            "https://play.google.com/store/apps"
        ));
        assert!(!sign_in(Service::Spotify, "https://notspotify.com/login"));
        assert!(!sign_in(
            Service::Spotify,
            "https://example.com/?accounts.google.com"
        ));
    }
}
