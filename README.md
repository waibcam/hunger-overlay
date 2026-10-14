# Hunger Overlay

**Your [Hunger Companion](https://hunger-companion.com) quest tracker, drawn directly on top of the in-game map of
*Hunger*.**

Open the map in game: your tracked quests, the containers and harvest nodes that drop what you need, spawn points,
extractions and more appear exactly where they are, and they follow the map as you zoom and pan.

- **One small `.exe`** (about 1.5 MB). Nothing to install, no runtime, no administrator rights.
- **About 10 MB of RAM** while the map is closed.
- **It reads the screen only.** It never reads or modifies the game's memory or files, never injects anything into the
  game and never simulates input (see [What it does, and what it doesn't](#what-it-does-and-what-it-doesnt)).

## Features

- **Automatic map detection.** No key binding is needed: the app recognises the map on screen, whichever way you
  opened it (map key, *Inspect* in the Compendium…). It also recognises which of the three maps is open (Sarlat Farm,
  Jacques Bridge, Sombre Forest).
- **Follows zoom and pan.** It follows the mouse wheel and dragging, then re-aligns itself on the image every time the
  map stops moving (typical error: 1–2 px). If it ever loses track, it hides the markers and finds the map again.
- **Your quests, from your account.** It shows the quests you track on the site, their remaining objectives, and where
  to go or what to loot.
  - For loot objectives, it shows the containers with their drop chance. A slider sets the minimum chance.
  - It shows harvest nodes, greyed out when they are above your declared gathering level.
- **Map panel.** You can move it and collapse it, and set its opacity. It also gives you:
  - quick settings;
  - one click to hide or show a quest;
  - optional **layers**: spawn points, extractions, places, harvest (filtered by your levels), placed loot, keys,
    locked doors and chests, mini-bosses, traps.
- **Find an item.** Type a name (in your language) and every place to find it appears on the map.
- **Find a quest.** Pick a quest in the same search window to start or stop tracking it on your account. This needs a
  key that is allowed to change your tracking.
- **Crafting list** (site beta). Recipes you want to craft appear like quests, with their ingredients' locations.
- **Ten languages** for the game data. The interface is in English or French; it follows your site account language,
  or you can force one.
- **Settings window** with a light or dark theme that follows Windows.
- **Automatic updates.** The new version is downloaded, its SHA-256 checked, and it is installed only when the map is
  not shown.

## Getting started

1. On hunger-companion.com, sign in, then go to **Account → Security → Overlay key → Generate**.
2. Download `HungerOverlay.exe` from the [latest release](https://github.com/waibcam/hunger-overlay/releases/latest)
   and run it. It is not signed yet: Windows SmartScreen may ask you to confirm (*More info → Run anyway*).
3. Paste your key in the settings window that opens, then **Save**.
4. Play in **borderless / windowed fullscreen** mode. Exclusive fullscreen hides every overlay.
5. Open the map in game.

The tray icon (next to the clock) gives access to the settings, item and quest search, a manual refresh, and Quit.

## What it does, and what it doesn't

| | |
|---|---|
| **Reads** | Screen pixels of the game window (GDI `BitBlt`), only to recognise the map and its position. The mouse wheel and button events (Raw Input, passive): they are never blocked or modified. |
| **Draws** | A transparent, click-through window above the game, plus a small panel that does not take focus. |
| **Sends** | Your API key, in an `Authorization` header over HTTPS, to `hunger-companion.com` only. It is never sent in a URL and never written to the log. When you pick a quest to (un)track, its quest id is sent too. No screenshots, input or game data are ever sent. |
| **Downloads** | Static JSON packs (quests, map calibrations, layers), your tracker, the items you search for, and updates listed in the site's `app.json`. An update is refused if its SHA-256 does not match. |
| **Stores** | `%APPDATA%\HungerOverlay\`: settings, a cache of the downloaded data, a short log. The API key is encrypted with Windows DPAPI. |
| **Never** | Reads the game's memory or files, injects code, simulates keys or clicks, or runs anything with administrator rights. |

Network usage is deliberately tiny:
- The quest data is fetched once per launch, with an ETag, so it is only downloaded when it changes.
- Your tracker is fetched when the map opens: at most once every 30 s and 100 times per hour. Nothing is polled in the
  background.

> **Easy Anti-Cheat:** the app belongs to the same category as external overlays (Discord, Overwolf…), and it does
> even less, since it injects nothing. It is not affiliated with or endorsed by the game's developer: use it at your own
> risk.

## Building from source

Requirements: Windows 10/11, the stable Rust toolchain (MSVC) and Python 3 with Pillow and NumPy (only to rebuild the
assets).

```bash
cargo build --release          # → target/release/HungerOverlay.exe
```

- `tools/make_assets.py <atlas>` rebuilds the embedded map reference textures and the icon.
- `tools/mock_site.py <export>` runs a local fake site implementing the API contract. To use it, set
  `"base_url": "http://127.0.0.1:8787"` in `%APPDATA%\HungerOverlay\config.json`.
- Tests:
  - `cargo test --release` runs the unit tests;
  - `HUNGER_RAWCAPS=<dir> cargo test --release real_captures -- --nocapture` replays real screenshots through the map
    recognition and re-alignment code.

### Source layout

| File | Role |
|---|---|
| `main.rs` | Application: windows, input, map state machine, drawing |
| `detect.rs`, `capture.rs`, `refs.rs` | Screen capture, map detection and identification, embedded references |
| `resync.rs` | Visual re-alignment (zoom and pan) on a background thread |
| `net.rs`, `model.rs` | Website API (contract v1.1) and data model |
| `panel.rs`, `render.rs` | Map panel and Direct2D rendering |
| `settings.rs`, `search.rs`, `tray.rs` | Settings window, search window, tray icon |
| `update.rs`, `config.rs`, `i18n.rs`, `log.rs` | Updates, settings file, texts, log |

## License

MIT. *Hunger* and its assets belong to their respective owners. This project is not affiliated with them.
