# Glowtune 0.5.5

> **Early build.** Glowtune is new and still has bugs. If something breaks, please tell us (see *Found a bug?* below).

> **🔊 Volume tip.** Turn the volume **all the way up inside Spotify, YouTube Music or SoundCloud**, then set how loud you want it with **Glowtune's** volume. With Automix, Crossfade or the Equalizer on, the two aren't linked.

## What's new

**Close the music window, keep the music.** Close the Spotify, YouTube Music, SoundCloud or Apple Music window while a song plays and the music keeps going in the background, using much less memory. Open it again from Glowtune whenever you want it back. Minimized, it stops drawing too.

**A lighter desktop scene.** When another window covers the scene (a maximized app or a game), it now goes fully to sleep and gives back its memory, then wakes the moment you can see it again.

**Lighter lyrics, same look.** The Stars, Orbit and Shards lyric styles, the floating desktop lyrics and the little equalizer bars now do their animation on the graphics card instead of redrawing the page every frame.

## Fixes

- With the music-video wallpaper, the animated colours hidden behind the video no longer keep running.
- The "What's new" card now shows reliably after an update.

## Known issues

- **The desktop scene still uses some CPU while it's on screen** (around 5–10% on a busy look), and its memory can climb for a few minutes before it drops back.
- **Apple Music** is barely tested.
- **Automatic lyrics timing** needs to hear the song start: if you skip into the middle of a song, use Later, Earlier or Sync.
- **The service's own volume slider and Glowtune's volume aren't linked** while Automix, Crossfade or the Equalizer is on.
- **The Clear player** catches clicks across its whole area, including the see-through parts.
- The installer isn't code-signed yet, so Windows SmartScreen shows a warning. Choose *More info › Run anyway*.

## Help wanted

We worked hard on memory and CPU in this release, but Glowtune draws its windows with WebView2 (a built-in Chrome engine), and that's where most of the memory goes. If you know a way to make Glowtune lighter on memory and CPU, we'd really appreciate your help: open an issue or a pull request on GitHub.

## Found a bug?

Please tell us what happened and what you were playing. A screenshot helps.

- In Glowtune: **Settings › Feedback**
- [Open an issue on GitHub](https://github.com/preetsaini2004/glowtune/issues/new)
- Email **Preet.saini.0@outlook.com**
