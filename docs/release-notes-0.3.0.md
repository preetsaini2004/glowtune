# Glowtune 0.3.0

> **Early build.** Glowtune is new and still has bugs. If something breaks, please tell us (see *Found a bug?* below).

> **🔊 Volume tip.** Turn the volume **all the way up inside Spotify, YouTube Music or SoundCloud**, then set how loud you want it with **Glowtune's** volume. With Automix, Crossfade or the Equalizer on, the two aren't linked.

## What's new

**Four new lyric styles** (scene menu › Lyrics):
- **Shards:** every letter flies in as broken slivers and snaps together just as it's sung, then the line shrinks away with ghost copies.
- **Tags:** the line hangs on swinging white tags, letters roll like a counter onto their words, and peel off at the end.
- **Code:** a row of glowing dots, one per letter, each turning into its letter as it's sung. Corner brackets close in at the end.
- **Melt:** letters brushed in with a streak of colour, running down the screen like wet paint when the line ends.

Shards, Code and Melt show the next line underneath and also work as floating desktop lyrics (Settings › Lyrics › Desktop look).

**Wallpapers.**
- **Dots** is new: glowing dots in the song's colours, with waves rolling out on the kick drum and calm in quiet parts.
- **Silk** is now sheer and takes the song's colours, so the cover shows through.
- **Mosaic** and **Silk** are now in the wallpaper menu.

**Clear player.** A mini player look with no window: the cover, the song and the controls float on your desktop (Settings › Player).

**A refreshed Store.** 29 themes and scene looks, 15 of them new, using the 0.2 glow shapes, particles, wallpapers and lyric styles. Also new: **Surprise me** and **Reinstall**.

**Resize the floating lyrics.** In Move lyrics, drag a corner of the box.

## Fixes

- **Up next** was empty with Spotify. It shows the queue again.
- After a song change, the progress bar stayed full and the lyrics froze until you skipped. Fixed, including with Automix.
- The desktop scene's seek bar can now be dragged, not just clicked.
- A theme you made could switch back to another one after a restart.
- **Use the theme's colors** off (it was called *Use a theme*) now keeps the theme's look and editor, and only lets the colours follow the album art.
- **Lyrics size** now works for the Stars and Rain floating lyrics too.
- **Corner radius** rounded a panel that the window's glass showed behind. It now rounds the cards, buttons and fields.
- The album **Glow** look washed out the song name. It's softer now, and the name sits above it.

## Known issues

- **Apple Music** is barely tested.
- **Music videos with a long intro** show the lyrics too early, because lyrics are timed to the album version. Pull requests welcome.
- **The service's own volume slider and Glowtune's volume aren't linked** while Automix, Crossfade or the Equalizer is on.
- **Switching your audio output device** while Sound effects are on can leave the effects off for up to a minute.
- **The Clear player** catches clicks across its whole area, including the see-through parts.
- The installer isn't code-signed yet, so Windows SmartScreen shows a warning. Choose *More info › Run anyway*.

## Found a bug?

Please tell us what happened and what you were playing. A screenshot helps.

- [Open an issue on GitHub](https://github.com/preetsaini2004/glowtune/issues/new)
- Email **Preet.saini.0@outlook.com**

Thanks to [JIZURA](https://github.com/852wa/JIZURA) (MIT) for the motion ideas behind the new lyric styles.
