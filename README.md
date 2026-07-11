# QuotaPeek

Small always-on-top desktop widgets (Windows + macOS) that pull **real** AI
subscription usage — for **Claude** and **OpenAI Codex** — and show, at a glance,
the current 5-hour session and weekly limits, mirroring each product's usage page.

Built with **Tauri v2** (Rust core + web UI). Tiny binary, low idle RAM.
Controlled from a **system tray** icon; each widget is a floating glass panel you
can show/hide independently.

## What it shows

**Claude widget** (blue, `Max (5x)`):
- **Current session (5-hour)** — % used + live "Resets in Xhr Ymin".
- **Weekly limits** — "All models" % used + "Resets Thu 12:00 PM", plus dynamic
  per-model rows (e.g. the rotating "Fable" codename).

**Codex widget** (green, `Plus`):
- **Current session (5-hour)** — % used + reset countdown.
- **Weekly limit** — % used.
- Bars turn amber/red as usage nears the limit (severity). Fetched live; a **stale**
  badge only appears if it falls back to the offline snapshot (network down).

Both: "Last updated" + manual refresh; re-auth and rate-limit banners.

## Tray + Settings

The tray icon (menu: **Settings**, **Reset positions**, **Check for updates**,
**Quit**) is the control center — left-click toggles the widgets, **Settings**
opens the panel, **Reset positions** snaps off-screen widgets back, **Quit** exits.
Closing the Settings panel only hides it; the app keeps running in the tray until
**Quit**.

**Settings** panel:
- Show Claude widget / Show Codex widget (independent)
- Start at login
- Always on top
- Refresh interval (30s / 1m / 1.5m / 2m / 5m)

Settings persist to `settings.json` in the app config dir.

## How it gets the data

- **Claude** — reuses the OAuth token Claude Code stores at
  `~/.claude/.credentials.json` and calls the same authoritative endpoint Claude
  Code's `/usage` uses: `GET https://api.anthropic.com/api/oauth/usage`. Read-only,
  no inference quota consumed. Fallbacks: unified rate-limit headers → transcript
  approximation. Never writes the credentials file — if the token is expired it
  shows a "re-auth needed — run `claude`" banner.
- **Codex** — live from the ChatGPT backend's **non-inference** usage endpoint
  `GET https://chatgpt.com/backend-api/wham/usage` (the same one the Codex CLI
  polls; **consumes zero model quota**), using the `access_token` + `account_id`
  from `~/.codex/auth.json` (read-only — the Codex CLI owns token refresh). If the
  network fails it falls back to the newest `rollout-*.jsonl` snapshot (marked
  stale); on 401 it shows "run `codex`".

> The Claude endpoint is undocumented. The widget polls infrequently (default 90s)
> with a `claude-code/*` User-Agent and backs off on 429 to stay within the
> rate-limit budget it shares with Claude Code.

## Run in dev

```bash
npm install
npm run tauri dev
```

## Build a release

```bash
npm run tauri build
```

Installer under `src-tauri/target/release/bundle/` (NSIS `.exe` on Windows,
`.dmg` on macOS).

## Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Parses captured real responses in `fixtures/` (`claude-usage.json`,
`codex-rollout.jsonl`) and asserts normalization (buckets, reset→epoch ms,
severity, credits) + the transcript-approximation walker.

## Project layout

```
index.html / settings.html    two frontend pages (Vite multi-page)
src/
  main.ts        widget: reads ?provider=, polls get_usage, countdown tick
  settings.ts    settings panel: toggles → set_* commands
  render.ts      builds the glass card from a snapshot (provider-aware)
  api.ts         invoke wrappers; types.ts mirrors the Rust structs
  style.css      dark-glass theme; codex = green accents; settings panel
src-tauri/src/
  lib.rs         tray (Settings/Quit), window setup, close→hide
  commands.rs    get_usage(provider), get_settings, set_show/autostart/aot/refresh
  claude.rs      Claude endpoint A→B→C
  codex.rs       Codex rollout-jsonl reader
  credentials.rs Claude token (read-only)
  models.rs      raw → normalized snapshot (+ tests)
  settings.rs    persisted Settings
  transcript.rs  Claude transcript approximation (Endpoint C)
fixtures/        captured real responses for tests
```

## Updates (auto-updater)

Wired via `tauri-plugin-updater`. The tray's **Check for updates** downloads,
installs, and restarts if a newer **signed** release exists at the configured
endpoint (`plugins.updater.endpoints` in `tauri.conf.json` → GitHub Releases
`latest.json`). To publish an update:

1. Bump `version` in `tauri.conf.json`.
2. Build with the signing key set (produces signed artifacts + `latest.json`):
   ```bash
   # PowerShell: set the key content from src-tauri/quotapeek-updater.key
   $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content src-tauri/quotapeek-updater.key -Raw
   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
   npm run tauri build
   ```
3. Upload the artifacts + `latest.json` to a GitHub Release on the repo the
   endpoint points to.

The private key `src-tauri/quotapeek-updater.key` is git-ignored — **keep it safe;
losing it breaks updates.** The public key is in `tauri.conf.json`.

## macOS note (written, UNTESTED — no Mac to verify)

- Claude token: read from the login **Keychain** (service `Claude Code-credentials`)
  as a fallback when the file isn't present (`credentials.rs`). The account name is
  a best guess (`$USER`) and may need adjustment on a real Mac.
- **Accessory** activation policy (no Dock icon / Cmd-Tab entry) — `lib.rs` setup.
- **Template** (monochrome) tray icon `icons/tray-template.png` for the menubar.
- Transparency uses `macOSPrivateApi`.

All macOS code is `cfg(target_os = "macos")`-gated (the Windows build is
unaffected), but it could not be compiled on Windows — verify on a Mac before
shipping.
