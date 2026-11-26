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
| Listening history (on by default; switch off or delete it in Settings → Listening) | `%APPDATA%\dev.preet.glowtune\listening.jsonl` | Song title, artist, app and how long it played, for your own stats page. Never sent anywhere. |
| Lyric cards you make (Ctrl+Alt+C) | `Pictures\Glowtune\` | Pictures you asked for |
| Scene presets | In your settings | The looks you save |

**Your logins:** you sign in on the service's own page (Spotify, Apple, Google / YouTube, SoundCloud), inside Glowtune.
- Glowtune doesn't read, store or send your password. The service's page handles it, as it would in a browser.
- Each service has its own profile, so one service can't see another's cookies.

**Sound:** for the beat-reactive glow, Glowtune listens to your PC's audio output while music plays.
- It measures loudness and rhythm in memory, and nothing else.
- Audio is never recorded, saved or sent anywhere.
- It doesn't use your microphone.

**Sound effects (optional, off by default):** when you turn on the equalizer or song blending, Glowtune takes the sound of its own service window from Windows, changes it, and plays the result.
- This happens live, in memory, on your PC. To blend one song into the next, a few seconds of sound are held in memory and then dropped.
- Nothing is recorded, saved to disk or sent anywhere.
- Only the sound of Glowtune's own service windows is touched, never other apps.
- While effects are on, Windows shows the service window's own volume as almost zero. That is expected: the sound you hear comes from Glowtune. Turning effects off puts it back.

**Lyrics timing:** when a song starts in another app (Spotify, your browser), Glowtune listens to that app's sound for its first few seconds, to hear where the singer comes in and line the lyrics up.
- It measures in memory and stops once it knows.
- Nothing is recorded, saved or sent anywhere, and the app's sound isn't changed.

## What goes online, and why

| Where to | What is sent | Why |
|---|---|---|
| **lrclib.net** | Song title, artist, album and length | To find synced lyrics. LRCLIB is a free, open lyrics database. |
| **itunes.apple.com** | What you type in search; the playing song’s title and artist; your top songs on the Listening page | Search results, a large copy of the album cover, and covers for your stats |
| The music service you open: **open.spotify.com**, **music.apple.com**, **music.youtube.com**, **soundcloud.com** (and SoundCloud's own API for autoplay suggestions) | Whatever that service's page sends | Same as using the service in a browser. Their own privacy policies apply. |
| **youtube-nocookie.com** | The video being shown | Music-video backgrounds on the desktop scene. YouTube's privacy-enhanced mode. |
| **easylist.to**, **ublockorigin.github.io** | Nothing about you (a plain file download) | Up-to-date ad-block filter lists |
| **github.com** (Glowtune's own releases page) | Nothing about you (a plain file download). Not in the Microsoft Store copy, which the Store updates. | To see if a new Glowtune version is out: when Glowtune starts and every 6 hours (switch off in Settings → About). If you press **Update now**, the new installer comes from the same place. |
| **api.web3forms.com**, **only when you press Send** in Settings → Feedback | What you wrote (your name or nickname, kind, part of Glowtune, title, message, and for a review whether it may be shown on Glowtune's website), your email if you typed one, and, if "Include app info" is on: Glowtune's version and where it was installed from, your Windows version, which music app is playing, and which features are on (glow, desktop lyrics, scene, sound effects). Settings shows the exact list before you send. | So your message reaches the developer. Web3Forms passes it on by email. Never your songs, history or anything else. |
| A Store catalog address, **only if you add one** in Settings → Store | Nothing about you (a plain file download) | Extra themes |

That's the complete list. Glowtune has no server of its own, and nothing is sent to its developers unless you send feedback yourself.

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
