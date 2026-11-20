# Card Conjurer Desktop

A Windows desktop app for the CardConjurer card creator, built with
[Tauri 2](https://tauri.app) (Rust backend, the system's WebView2 for the UI).

- **Offline:** frames, fonts and scripts are served from disk through the
  app's own `cc://` URL scheme. Only the optional Scryfall import and
  set-symbol lookups use the internet.
- **No 5 MB limit:** saved cards are files on disk, not browser localStorage.
- **Creator only:** the home page, menu and other tools are left out.

## Requirements (Windows)

1. Rust via [rustup](https://rustup.rs), MSVC toolchain (the default on Windows).
2. Visual Studio Build Tools with the "Desktop development with C++" workload.
3. WebView2, which is already installed on Windows 10 and 11.

## Run and build

From `desktop/src-tauri`:

```
cargo run                 # debug build, opens the app
cargo build --release     # optimized build: target/release/cardconjurer-desktop.exe
cargo test                # storage and file-serving tests
```

No Node.js or Tauri CLI needed. (The Tauri CLI, `cargo install tauri-cli`,
only matters later for making an installer.)

## Packaging

From `desktop`, in PowerShell:

```
.\package.ps1          # builds desktop\release\Card Conjurer\
.\package.ps1 -Zip     # same, plus Card Conjurer.zip
.\package.ps1 -SkipBuild   # refresh the assets only
```

The result is a portable folder: `Card Conjurer.exe` plus an `assets` folder
holding only what the creator uses (css, js, img, fonts, creator, core,
local_art, desktop\web). Release builds don't open a console window. Run the
exe from anywhere; it doesn't need installing. Windows 10 and 11 already have
WebView2. Saved cards stay in `%APPDATA%`, so replacing the folder with a newer
build keeps them.

If PowerShell refuses to run the script, run it once with:
`powershell -ExecutionPolicy Bypass -File .\package.ps1`

The app finds the CardConjurer files by looking, in order, at:

1. the `CARDCONJURER_ASSETS` environment variable,
2. the folder the .exe is in, its parent folders, and an `assets` folder next to any of them,
3. the repo it was built from.

So running the .exe from `target/release` inside the repo just works.

## How it fits together

| Path | What it does |
| --- | --- |
| `src-tauri/src/main.rs` | Opens the window, registers the `cc://` scheme and the card commands. |
| `src-tauri/src/assets.rs` | Serves repo files; `/` is `web/shell.html` with `creator/index.html` inserted. |
| `src-tauri/src/cards.rs` | Saved cards, one JSON file each. |
| `web/shell.html` | The page around the creator (styles, theme, notifications). |
| `web/card-store.js` | `cardStore`, used by `js/creator-23.js` for saving and loading cards. |
| `web/history.js` | Undo/redo: snapshots of `card` after each edit, plus the bar under the preview and Ctrl+Z / Ctrl+Y. |
| `web/desktop.css` | Styles for the desktop-only additions. |

Changes to `js/creator-23.js` are marked `// DESKTOP`. They replace its
localStorage card storage with `cardStore`. Settings such as auto-fit
still use localStorage, since they're tiny.

## Saved cards

Saved cards are stored in `%APPDATA%\com.cardconjurer.desktop\cards\`, one
`.json` file per card. Each file has the same `{"key": ..., "data": ...}`
shape as an entry in the website's "Download All" export.

To bring cards over from the website: on the website, use
**Download All** under saved cards. Then in the app, choose that file with
the upload button under **Download All**.
