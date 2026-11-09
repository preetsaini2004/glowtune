# Privacy

**Short version:** Glowtune runs entirely on your PC. It has no account, no server, no analytics, no ads and no tracking. It never sees your passwords. The only things that go online are listed below, each with the reason.

## What Glowtune keeps on your PC

| What | Where | Why |
|---|---|---|
| Settings and your themes | `%APPDATA%\dev.preet.glowtune\` | Your choices, and themes you made or installed |
| Lyrics cache | `%LOCALAPPDATA%\dev.preet.glowtune\lyrics\` | So a song's lyrics load instantly the next time |
| Ad-block filter lists, Store packs, small diagnostic notes | `%LOCALAPPDATA%\dev.preet.glowtune\` (cache) | Ad blocking; downloaded themes; notes that help fix a service whose website changed. They stay on your PC unless you choose to share them. |
| Your music-service logins | `%LOCALAPPDATA%\dev.preet.glowtune\services\<service>\` | Each service runs in its own web profile, like a separate browser, so you stay signed in |
| Recently played | Memory only | Shown on the desktop scene; forgotten when Glowtune closes |

**Your logins:** you sign in on the service's own page (Spotify, Apple, Google / YouTube, SoundCloud), inside Glowtune.
- Glowtune doesn't read, store or send your password. The service's page handles it, as it would in a browser.
- Each service has its own profile, so one service can't see another's cookies.

**Sound:** for the beat-reactive glow, Glowtune listens to your PC's audio output while music plays.
- It measures loudness and rhythm in memory, and nothing else.
- Audio is never recorded, saved or sent anywhere.
- It doesn't use your microphone.

## What goes online, and why

| Where to | What is sent | Why |
|---|---|---|
| **lrclib.net** | Song title, artist, album and length | To find synced lyrics. LRCLIB is a free, open lyrics database. |
| **itunes.apple.com** | What you type in search | Search results and cover art |
| The music service you open: **open.spotify.com**, **music.apple.com**, **music.youtube.com**, **soundcloud.com** (and SoundCloud's own API for autoplay suggestions) | Whatever that service's page sends | Same as using the service in a browser. Their own privacy policies apply. |
| **youtube-nocookie.com** | The video being shown | Music-video backgrounds on the desktop scene. YouTube's privacy-enhanced mode. |
| **easylist.to**, **ublockorigin.github.io** | Nothing about you (a plain file download) | Up-to-date ad-block filter lists |
| A Store catalog address, **only if you add one** in Settings → Store | Nothing about you (a plain file download) | Extra themes |

That's the complete list. Glowtune has no server of its own, and nothing is sent to its developers.

The source code is public: you can check every network call yourself (`git grep -nE "https?://"`), or watch the app's traffic with any network monitor.

## Ad blocking
Ads are blocked only inside Glowtune's own windows. Glowtune doesn't change your browser, your system or any other app.

## Music from other apps
Glowtune reads what's playing from Windows' media controls: the title, artist, album, cover and position that apps like Spotify or your browser already share with Windows. That information stays on your PC, except the lyrics lookup described above.

## Delete everything
1. Uninstall Glowtune: Windows Settings → Apps → Installed apps → Glowtune.
2. Delete the folders `%APPDATA%\dev.preet.glowtune` and `%LOCALAPPDATA%\dev.preet.glowtune`. This also signs you out of the services inside Glowtune.

## Questions
Open an issue on GitHub. For anything security-related, see [SECURITY.md](SECURITY.md).
