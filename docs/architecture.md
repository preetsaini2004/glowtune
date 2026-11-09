# How Glowtune is built

Glowtune is a Rust workspace with a [Tauri 2](https://tauri.app) app and a [Svelte 5](https://svelte.dev) interface. Windows is the main platform; the Linux backends compile but haven't been tested on a real desktop yet.

```text
 any app playing music ──► Windows media controls (SMTC)
                                   │
                           crates/media_source
                                   │  track, cover, play state, position
                                   ▼
                     app/src-tauri  ── hub (one place for "now playing")
          ┌──────────────┬─────────┴───────┬──────────────────┬───────────────┐
          ▼              ▼                 ▼                  ▼               ▼
   crates/analysis  crates/lyrics     crates/overlay     Svelte windows   service windows
   cover colors,    LRCLIB + .lrc,    edge glow          player, scene,   Spotify / Apple /
   beat (from       cache, sync       (wgpu, DX12)       lyrics,          YouTube Music /
   audio_capture)   clock                                settings         SoundCloud + ad block
```

## Crates (`crates/`)

| Crate | What it does |
|---|---|
| `media_source` | Now-playing info from the OS: SMTC on Windows, MPRIS on Linux. Sends play / pause / next / seek. |
| `analysis` | Album-art colors (OKLab k-means) and beat / onset detection (FFT). |
| `audio_capture` | System audio loopback ("what you hear", never the microphone), used for the beat. |
| `lyrics` | LRCLIB client, `.lrc` parser, on-disk cache and the lyrics sync clock. |
| `overlay` | The native screen-edge glow window (winit + wgpu). Hides over full-screen games. |
| `themes` | Theme files (TOML): schema, validation and hot reload. Bundled themes live in `themes/`. |
| `adblock_layer` | Network and cosmetic filtering for the service windows (Brave's adblock engine), with a weekly filter-list cache. |
| `library` | Local music index with full-text search. Not wired into the app yet. |

## The app (`app/`)

| Where | What |
|---|---|
| `app/src-tauri/src/lib.rs` | Startup, tray menu, commands the interface calls |
| `app/src-tauri/src/hub.rs` | Collects media events, colors, lyrics, history; tells every window |
| `app/src-tauri/src/glow.rs` | Feeds colors and beat energy to the edge glow |
| `app/src-tauri/src/stage.rs` | The desktop scene windows (one per screen with All screens) |
| `app/src-tauri/src/services.rs` | Service windows: one profile per service, sign-in popups, play / pause through the page's own buttons |
| `app/src-tauri/src/music_browser.rs`, `music_snapshot.js`, `soundcloud_station.js` | Reads shelves and the Up next queue from the service pages |
| `app/src-tauri/src/adblock.rs`, `youtube_ads.js` | Hooks the ad blocker into the service windows |
| `app/src-tauri/src/thumbbar.rs` | Previous / play / next buttons under the taskbar preview |
| `app/src-tauri/src/store.rs` | The theme Store |
| `app/src/routes/` | Svelte pages: `+page` (player), `stage/` (desktop scene), `lyrics/` (desktop lyrics), `settings/` |
| `app/src/lib/` | Shared Svelte components (lyrics, dock, search panel, Up next…) |

## Rules the code follows

- **Idle means idle.** Nothing polls while no music plays; CPU stays near 0 %.
- **Web pages get no app powers.** Service windows are left out of every Tauri capability (`app/src-tauri/capabilities/default.json`), so remote pages can't call app commands.
- **Async commands for windows.** Tauri commands that create windows must be `async`; synchronous ones deadlock on Windows.
- **Services change their pages.** All selectors for a service live in `services.rs`, `music_snapshot.js` and `music_browser.rs`, and fail soft (an empty list, never a crash).

## Gotchas

- A window at negative monitor coordinates needs `PhysicalPosition` after `build()`.
- Only one service window exists at a time. Its WebView2 profile can stay busy for a moment after closing (error `0x800700AA`); the code retries.
- The "is the scene covered?" check ignores invisible layered windows (the NVIDIA overlay, Rainmeter widgets).

## Checking your change

```sh
./scripts/check.sh        # fmt, clippy (warnings are errors), tests, svelte-check, build
./scripts/check.sh rust   # Rust only
./scripts/check.sh ui     # interface only
```
