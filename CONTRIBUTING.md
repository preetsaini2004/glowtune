# Contributing to Glowtune

Thanks for helping. Bug reports, fixes, themes and ideas are all welcome.

## Report a bug
[Open an issue](../../issues/new/choose) using the bug form. A screenshot plus "what I did, what happened, what I expected" is usually enough to fix it. Music services change their websites often, so reports about Spotify, Apple Music, YouTube Music or SoundCloud are especially useful.

## Make a theme
Themes are small TOML files. Copy one from [`themes/`](themes/), change the colors and effects, and drop it in `%APPDATA%\dev.preet.glowtune\themes\`. Glowtune reloads it as you save. To share it with everyone, open a pull request that adds it to `themes/`.

## Change the code

1. Install [Rust](https://rustup.rs) (stable), [Node.js](https://nodejs.org) 20+ and [pnpm](https://pnpm.io). On Windows, also install the Visual Studio C++ build tools.
2. Run the app with hot reload:
   ```sh
   cd app
   pnpm install
   pnpm tauri dev
   ```
3. Read [docs/architecture.md](docs/architecture.md) to find your way around.
4. Before opening a pull request, make sure everything passes:
   ```sh
   ./scripts/check.sh
   ```

### Fixing a music service
When a service changes its page, its buttons or lists can stop working. The selectors for each service live in:
- `app/src-tauri/src/services.rs`: play / pause / next / previous, shuffle, repeat, like and volume
- `app/src-tauri/src/music_snapshot.js`: shelves, the Up next queue and the current song (with tests in `music_snapshot.test.cjs`)
- `app/src-tauri/src/music_browser.rs`: opening the player's queue panel

Keep selectors failing soft: an empty result, never an error the user sees.

### Guidelines
- Keep idle CPU near zero: no polling while nothing plays.
- Never give remote web pages access to app commands.
- Never send user data anywhere new. If a change adds a network request, add it to [PRIVACY.md](PRIVACY.md) in the same pull request.
- Small pull requests with a clear description are easiest to review.
- Write commit messages that say what changed for the user, for example "SoundCloud: volume slider works".

## Code of conduct
Be kind, assume good intent, and keep discussions about the work.
