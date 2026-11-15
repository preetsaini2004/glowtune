# Glowtune 0.2.0

> **Early build.** Glowtune is new and still has bugs. Some things may glitch, and Apple Music and SoundCloud are barely tested. If something breaks, please tell us (see *Found a bug?* below).

> **🔊 Volume tip.** Turn the volume **all the way up inside Spotify, YouTube Music or SoundCloud**, then set how loud you want it with **Glowtune's** volume (the dock, mini player or Ctrl+Alt+↑/↓). With Automix, Crossfade or the Equalizer on, Glowtune's volume and the service's own slider are separate and don't follow each other, so a low service slider makes everything quieter.

## What's new

**Scene presets.** Set up the desktop scene how you like it (wallpaper, lyrics, album art, dock, glow, where everything sits), give it a name and save it. One click, or **Ctrl+Alt+1–9**, brings it back.

**Hotkeys.** Control the music from any app, even in a game:

| Ctrl+Alt + | Does |
|---|---|
| Space | Play / pause |
| ← / → | Previous / next song |
| ↑ / ↓ | Volume |
| L | Desktop lyrics on / off |
| P | Mini player |
| D | Desktop scene on / off |
| S | Sleep timer |
| C | Lyric card |
| N | Next preset |
| 1–9 | Your presets |

A small toast shows what each key did. On keyboards where Ctrl+Alt types characters (AltGr), those keys are left free for typing.

**Sleep timer.** Pause the music after 15, 30, 45 or 60 minutes, or when the current song ends.

**Lyric cards.** Press **Ctrl+Alt+C** to save the line being sung, with the cover and the album's colours, as a picture in *Pictures › Glowtune*.

**Listening stats.** Settings › Listening shows your time listened, top songs and artists, and when you listen most. It stays on your PC, and you can switch it off or delete it.

**Lyric styles.** Stars (each word a star, joined as it's sung), Depth, Rain and Orbit, plus emoji bursts on words like *money* or *heart*.

**Wallpapers.** Album art (slowly moving), Cover, Water, Frost and Halftone. Water, Frost and Halftone pulse with the beat. Move the mouse over Water or Frost to wipe it clear.

**Album and dock looks.** The album can be Tilt, Print, Vinyl (the cover spins as a record) or Glow. The dock can be Bar, Pill, Islands or Line.

**Edge glow.** Five new shapes (Comets, Aurora, Equalizer, Neon, Tide) and four new particles (Embers, Snow, Fireflies, Bubbles), picked from the scene menu.

**Mini player.** Five looks (Glass, Disc, Card, Pill, Lyric), three sizes, and a volume control (button or mouse wheel).

**Taskbar player.** A slim player on the left or right end of the taskbar.

**Desktop lyrics over games.** Floating lyrics can stay up over borderless full-screen games, in the Theme, Stars or Rain look.

**Sharper covers.** A large copy of the album cover is fetched for the scene.

## Fixes

- The glow could slip under other windows and only show through the taskbar.
- Automix song changes no longer freeze the scene, lyrics and dock.
- Karaoke lyrics no longer get stuck between two styles.
- Lyrics are no longer looked up for ads.

## Known issues

- **Apple Music and SoundCloud** are barely tested.
- **Music videos with a long intro** (talking or a skit before the song) show the lyrics too early, because lyrics are timed to the album version. Help wanted: if you can make Glowtune detect where the singing starts, please open a pull request. We'll test it together and credit you as a co-author.
- **The service's own volume slider and Glowtune's volume aren't linked** while Automix, Crossfade or the Equalizer is on. Keep the service at full volume and use Glowtune's volume. Pull requests to link them are welcome.
- **Switching your audio output device** (speakers ↔ headphones) while Sound effects are on can leave the effects off for up to a minute before they recover.
- **Automix** plays the sound about half a minute behind, so after starting, skipping or seeking it needs a moment to build up again.
- One crash was seen once when switching to YouTube Music, and it hasn't come back. If it happens to you, please report it.
- The installer isn't code-signed yet, so Windows SmartScreen shows a warning. Choose *More info › Run anyway*.

## Found a bug?

Please tell us what happened and what you were playing. A screenshot helps.

- [Open an issue on GitHub](https://github.com/preetsaini2004/glowtune/issues/new)
- Email **Preet.saini.0@outlook.com**

You can also use the buttons in Settings › About.
