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
- **Usage credits** — amount spent / enabled state.

**Codex widget** (green, `Plus`):
- **Current session (5-hour)** — % used + reset countdown.
- **Weekly limit** — % used.
- Bars turn amber/red as usage nears the limit (severity). A **stale** badge shows
  when the data is old (Codex numbers only refresh when you run `codex`).

Both: "Last updated" + manual refresh; re-auth and rate-limit banners.

## Tray + Settings

The tray icon (menu: **Settings**, **Quit**) is the control center — left-click
toggles the widgets, **Settings** opens the panel, **Quit** exits. Closing the
Settings panel only hides it; the app keeps running in the tray until **Quit**.

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
- **Codex** — reads the newest `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` and
  takes the last `rate_limits` snapshot (offline, no network). Only as fresh as the
  last Codex turn → staleness badge.

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

## macOS note

On macOS the Claude token may live in the Keychain instead of the file; a Keychain
read path is a planned addition for the mac build. Transparency uses
`macOSPrivateApi`.
