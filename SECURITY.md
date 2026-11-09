# Security

## Reporting a vulnerability

Please **don't** open a public issue for security problems. Use GitHub's private reporting instead: **Security → Report a vulnerability** on this repository. Include what you found, how to reproduce it, and what it could let someone do.

You'll get an answer as soon as possible. Once a fix is released, you can be credited in the release notes if you like.

## Supported versions

Only the latest release gets security fixes. The app doesn't update itself yet, so please install new releases from the Releases page.

## How Glowtune is built to stay safe

- **Websites can't control the app.** The music services run in their own windows. Those windows have no access to Glowtune's internal commands (they're in no Tauri capability), so a web page can't read your files or change settings.
- **One profile per service.** Each service has its own web profile, so cookies and logins never mix between services.
- **Popups are limited.** Sign-in popups (Google, Apple, Facebook, Microsoft, or the service itself) are allowed. Any other new window opens in your normal browser instead of inside Glowtune.
- **Downloads are treated as data.** Store packs and filter lists are size-limited and parsed, never run. Themes are plain settings files (TOML) and can't run code.
- **No secrets in the app.** Glowtune ships no API keys or tokens, and has no server.
- **Release builds are made by GitHub Actions** from the public source code. Each release lists the SHA-256 fingerprint of the installer so you can check your download.
