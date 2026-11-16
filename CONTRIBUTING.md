# Contributing

Thanks for helping. The project's promise is **simple tools that do what you
want, with no frills**, so the best contributions are often the smallest.

## Ideas and bug reports

[Open an issue](../../issues/new/choose). For an idea, describe the annoyance
you want gone, not only the feature. "Every time I plug in my headphones,
Windows switches the output and I have to switch it back" helps more than
"add an audio tool".

## How it fits together

```
src/
  main.rs           start-up: one copy only, admin rights, tray and window, installer steps
  desktop.rs        tray icon and the WebView2 window; serves the UI over a private spt:// protocol
  api.rs            the UI's routes: page files, info, settings, updates; mounts the tools' APIs
  settings.rs       settings.json (which tools are on, theme, update choices)
  autostart.rs      the "Simple PC Tools" sign-in task (Task Scheduler)
  elevation.rs      the admin rights check and relaunch
  instance.rs       the one-copy rule (a local port that accepts only SHOW and QUIT)
  update.rs         the GitHub release check and the update download
  tools/
    mod.rs          THE TOOL LIST (ALL), and starting / routing / switching tools
    apps/           Apps: model (mod.rs), API (api.rs), check loop (supervisor.rs),
                    checks (probe.rs), start/stop steps (action.rs), starting with
                    the user's normal rights (launch.rs)
    disk.rs         Disk usage API, on top of crates/disk-scan
crates/disk-scan/   fast folder sizes: reads the NTFS file table, or walks the folders
ui/
  index.html        the shell: sidebar, icon sprite, script list
  style.css         all styles; design tokens at the top (DESIGN.md)
  js/core.js        helpers, API client, page registry, nav, routing, update banner
  js/<id>.js        one page per tool (apps.js, disk.js), then settings.js
  js/demo.js        demo data for /?demo (every state of every page)
installer/          the Inno Setup wizard
scripts/            screenshots.ps1 (README pictures from the demo data)
tests/ui_design.rs  design rules, and checks that every tool is fully wired
```

The UI has no framework and no build step: plain HTML, CSS and JavaScript,
built into the exe with `include_str!`.

The app runs as administrator, so **its API must never be reachable from
outside its own window**. The window gets the page and API through the private
`spt://` protocol, not a network port. `--dev-server` exists for development
only.

## Adding a tool

First ask: **is it simple?** One job, useful with its defaults, and nothing to
learn first. Power-user options go under an **Advanced** disclosure.

Say the new tool is `cleanup`.

**Server (Rust)**

1. Write `src/tools/cleanup.rs` with `pub fn router(...) -> axum::Router` (see
   `disk.rs` for a small example) and `pub mod cleanup;` in `src/tools/mod.rs`.
2. In `src/tools/mod.rs`, add it to **`ALL`** (id, name, one-sentence about),
   and nest its router in `Tools::router` under `/api/cleanup`. If it does
   background work, keep its state in `Tools` and pause it in `set_enabled`.

That's all on the server. Settings, `--tools`, the Settings switch and the
nav entry all come from `ALL`.

**Page (JavaScript)**

3. Write `ui/js/cleanup.js`, starting with
   `const cleanupPage = registerPage('cleanup', { show: showCleanup });`.
   Render into `cleanupPage`, reuse the components in DESIGN.md, and use
   `api()`, `run()`, `toast()` and `confirmClick()` from `core.js`.
4. Load it in `ui/index.html` (before `settings.js`), serve it in `UI_FILES`
   in `src/api.rs`, and add it to `scripts()` in `tests/ui_design.rs`.
5. Add a nav icon `<symbol id="i-cleanup">` to the sprite in `ui/index.html`.
6. In `ui/js/demo.js`, add the tool to `demo.info.tools` and give `demoApi`
   answers for `/api/cleanup/...`.

**Installer and docs**

7. Add a `[Components]` line `Name: "cleanup"` to `installer/simple-pc-tools.iss`.
8. Add a README section and run `scripts\screenshots.ps1` for a screenshot.

`cargo test` fails with a message naming the missing piece if you skip any of
steps 2–7.

## Before opening a pull request

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For UI changes, also do the visual check in DESIGN.md ("Checking a change").
Run a **development copy** (`--no-elevate --dev-server … --instance-port 47699
--data-dir target\dev-data`). Never test against your real install: a second
copy sharing its data would start and restart your real programs.

## Releasing (maintainers)

1. Bump `version` in `Cargo.toml` and add an entry to `CHANGELOG.md`.
2. Commit, then tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The release workflow tests the code, builds the installer and a portable
   zip, and publishes the release. Installed copies offer the update within a
   day.

By contributing, you agree that your contribution is licensed under
GPL-3.0-or-later.
