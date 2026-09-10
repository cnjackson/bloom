# Bloom

A system-wide text-replacement expander for Windows 11. Bloom watches
keystrokes globally and expands triggers into replacements as you
type.

**Why this exists:** typing common phrases over and over is tedious.
Define a short trigger once; Bloom types the long version for you,
everywhere on the system, in any app. It's particularly useful for
AI-prompt workflows in development: a trigger like `;code` expands to
the full "Act as a senior software engineer" system prompt; a trigger
like `/pr` becomes a structured PR-description template; `!!clarify`
becomes a meta-prompt that asks the model to interrogate ambiguous
input. See [Sample data: AI prompt library](#sample-data-ai-prompt-library)
for a starter set of 23 rules tailored to this.

## Install

### MSI (recommended, per-user, no UAC)

1. Download `Bloom_0.1.0_x64_en-US.msi` from the release you want (or
   build it - see [Rebuild from source](#rebuild-from-source)).
2. Run the MSI. The installer uses the per-user install scope - no
   admin prompt, no UAC, no `Program Files`.
3. Bloom is installed to `%LOCALAPPDATA%\Programs\Bloom\`.
4. A Start menu shortcut is created (`Bloom.lnk`).
5. **Settings → Apps → Installed apps** lists Bloom (via a per-user registry entry).
6. Launch Bloom from the Start menu or the system tray icon.

To uninstall: Settings → Apps → Bloom → Uninstall. (Or run the
uninstall helper installed at
`%LOCALAPPDATA%\Programs\Bloom\uninstall.ps1`.)

### NSIS installer (alternative)

`Bloom_0.1.0_x64-setup.exe` - same payload as the MSI, different
installer framework.

## Use

Open Bloom from the tray (single-click) and:

- **+ Add rule** - create a trigger + replacement
- **Click a row's Edit** - opens the rule editor with a large text area
  for multi-line replacements
- **Click the Trigger header** - sort A→Z / Z→A / insertion order
- **Settings cog (top-right)** - theme (Dark / Light / System), a
  "Show debug log" toggle, and an "About" tab with version, install
  path and config path

When the app is running, type a trigger in any non-blacklisted app
followed by whitespace. Bloom expands it in-place:

    answer short [space]   ->  Answer in short in plain English.
    ;test [space]          ->  (the AI-prompt fixture body)
    /pr [space]            ->  (the AI-prompt fixture body)

Triggers fire on the whitespace boundary. The trailing whitespace
itself is preserved.

## Settings that aren't in the UI

These are stored in `%APPDATA%\bloom\rules.json` and can be edited by
hand (with the app closed):

```json
{
  "version": 1,
  "start_with_windows": false,
  "blacklist": ["password.exe", "1password.exe"],
  "scoped_to": null,
  "theme": { "mode": "system", "custom": null },
  "show_debug_log": false,
  "rules": [
    {
      "id": "01HMRX...",
      "trigger": "answer short",
      "replacement": "Answer in short in plain English.",
      "enabled": true,
      "created_at": "2026-09-04T..."
    }
  ]
}
```

- **`blacklist`** - per-global exe filter. Don't expand in these apps.
- **`scoped_to`** - the inverse: only expand in these exes. `null`
  means "everywhere except the blacklist".
- **`show_debug_log`** - opt-in in-page debug overlay (visible in
  Bloom's main window, not in the focused app).

## Sample data: AI prompt library

`fixtures/ai-prompt-library.json` ships 24 rule bodies that turn
short prefixes into full system-prompt templates. Useful for any
flow that sends repeated text to a chat assistant, code review tool,
or IDE-copilot:

- **17 role prompts** (`answer short`, `;code`, `;design`, `;review`,
  `;spec`, `;refactor`, `;test`, `;sec`, `;sys_agent`, `;sys_reviewer`,
  `;sys_architect`, `;ctx_stack`, `;ctx_repo`, `;go`, `;simplify`,
  `;gitpush`, `;short`)
- **4 task prompts** (`/pr`, `/test`, `/sec`, `/perf`)
- **3 meta prompts** (`!!clarify`, `!!think_harder`, `!!critique`)

Three prefix conventions so triggers don't collide with each other:

- `;name` — adopts a role ("act as …")
- `/name` — runs a structured task
- `!!name` — applies a meta-instruction to the next thing you type

To import: Bloom → **Import** button → pick the file. The import
resolves trigger conflicts via a per-rule modal (Overwrite / Skip).

A copy is also at `~/Downloads/bloom-prompt-library.json` for quick
testing.

**Adding your own:** any rule whose body contains `{{placeholder}}`
text will paste it back literally — Bloom does not substitute. So a
trigger like `;bugfix` with body

```
Fix the following bug in {{REPO}}:

{{BUG_DESCRIPTION}}

Output: a unified diff against {{BASE_BRANCH}}.
```

will type the placeholder strings back into whatever app you're in,
ready for you to fill in.

## Behavior you should know

- **Trigger boundary is whitespace.** A trigger fires when followed by
  space, tab, newline, or any whitespace transition.
- **Multi-word triggers are fine.** `answer short` works.
- **Triggers with punctuation prefixes work.** `;test`, `/pr`, `!!think_harder`
  - the hook captures OEM punctuation from the focused app.
- **`scoped_to` is opt-in.** Empty/null means "fire everywhere except
  the blacklist". A non-empty list is a hard filter.
- **Expansion is silent.** No toast, no log on the focused app - the
  replacement just appears.
- **Backspace is safe.** Typing a trigger, hitting Backspace, then
  Space won't expand the partial.
- **Buffer survives arrow keys / typing in the middle.** Bloom keeps
  the trigger suffix when you navigate.
- **Expansion puts the original clipboard back 120ms after pasting.**
  Clipboard managers that take ownership during that window can drop
  the paste - don't type fast expansions if you're juggling a
  clipboard manager.

## Architecture

Bloom is a Tauri 2 app. The Rust core owns the global keyboard hook
(`SetWindowsHookExW(WH_KEYBOARD_LL)` via windows-rs 0.62), a typed
character buffer, the rules store, and a JSON over IPC for the
frontend. The frontend is plain HTML/JS - no React, no build step
inside `dist/`. Plugins: `tauri-plugin-autostart`,
`tauri-plugin-clipboard-manager`, `tauri-plugin-dialog`,
`tauri-plugin-fs`, `tauri-plugin-path`. The architecture of every
component lives next to the code it describes (see
[Configuration](#configuration) for the file map).

## Rebuild from source

### Toolchain

- Rust 1.96+ (stable, msvc toolchain)
- Node 22+ (only for `cargo tauri` CLI helpers; no JS in the app)
- MSVC 19.44+ with Windows SDK 10.0.19041
- WebView2 runtime 152.x (pre-installed on Windows 11)
- `cargo-tauri` CLI 2.11+ (`cargo install tauri-cli@^2`)

Verify with:

```bash
rustc --version && cargo --version && node --version
```

### Build the release MSI + NSIS

```bash
cd src-tauri
cargo install tauri-cli@^2 --version "^2.11"   # one-time
cargo tauri build
```

Output:

```
src-tauri/target/release/bloom.exe                              (5.0 MB)
src-tauri/target/release/bundle/msi/Bloom_0.1.0_x64_en-US.msi   (4.4 MB)
src-tauri/target/release/bundle/nsis/Bloom_0.1.0_x64-setup.exe
```

The `[profile.release]` is already tuned for size:

```toml
codegen-units = 1
lto = true
opt-level = "s"
panic = "abort"
strip = true
incremental = false
```

If you change it and rebuild, expect the exe to balloon back up to
~13 MB.

### Install the freshly-built exe

```bash
scripts/install-per-user.ps1
```

That:

1. Kills the running `bloom.exe`
2. Copies `target/release/bloom.exe` to
   `%LOCALAPPDATA%\Programs\Bloom\bloom.exe`
3. Creates the Start menu shortcut
4. Adds the per-user `HKCU\Uninstall\…` registry entry so Settings →
   Apps lists Bloom

### Uninstall

```bash
scripts/uninstall-per-user.sh
# (or %LOCALAPPDATA%\Programs\Bloom\uninstall.ps1 from the install dir)
```

Removes the process, install dir, Start menu shortcut, autostart
registry, and the Settings→Apps entry.

## Development loop

```bash
cd src-tauri
cargo tauri dev
```

- Edits to `src-tauri/src/*.rs` trigger Rust rebuilds.
- Edits to `dist/*.html / *.js / *.css` hot-reload inside the
  WebView.
- The debug log toggle in Settings can be enabled without a restart.

## Configuration

| File | Purpose |
|---|---|
| `src-tauri/src/lib.rs` | App entry, plugin wiring, setup |
| `src-tauri/src/hook.rs` | `WH_KEYBOARD_LL` install + buffer state + match |
| `src-tauri/src/store.rs` | Atomic JSON read/write + schema versioning |
| `src-tauri/src/commands.rs` | Tauri commands (save_all, import, export, ...) |
| `src-tauri/src/model.rs` | Config / Rule / Theme types |
| `src-tauri/src/tray.rs` | Tray icon + single-click → window |
| `src-tauri/tauri.conf.json` | Bundle config, identifier `dev.cnjackson.bloom` (do not change without a Settings→Apps migration plan) |
| `dist/index.html` | Single-file HTML+SVG+CSS shell |
| `dist/app.js` | Plain JS frontend (no build step) |

## License

This is a personal project. No license is granted for redistribution,
modification, or derivative works.
