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
  main.rs         start-up: one copy only, admin rights, tray and window, installer steps
  desktop.rs      tray icon and the WebView2 window; serves the UI over a private spt:// protocol
  api.rs          the UI's routes: page, settings, updates; each tool's API is nested in
  settings.rs     settings.json (which tools are on, theme, update choices)
  autostart.rs    the "Simple PC Tools" sign-in task (Task Scheduler)
  elevation.rs    the admin rights check and relaunch
  instance.rs     the one-copy rule (a local port that accepts only SHOW and QUIT)
  update.rs       the GitHub release check and the update download
  tools/
    apps/         App manager: model (mod.rs), check loop (supervisor.rs), checks (probe.rs),
                  start/stop steps (action.rs), starting with the user's normal rights (launch.rs)
    disk.rs       Disk usage API on top of crates/disk-scan
crates/disk-scan  fast folder sizes: reads the NTFS file table, or walks the folders
ui/index.html     the whole UI: one file with no build step (see DESIGN.md)
installer/        the Inno Setup script
```

The app runs as administrator, so **its API must never be reachable from
outside its own window**. The window gets the page and API through the private
`spt://` protocol, not a network port. `--dev-server` exists for development
only.

## Adding a tool

1. **Is it simple?** One job, useful with its defaults, and nothing the user
   has to learn first. Power-user options go under **Advanced**.
2. **Backend.** Add `src/tools/<tool>.rs` (or a folder) with a
   `router() -> axum::Router`, and nest it in `src/api.rs` under
   `/api/<tool>`.
3. **On/off.** Add a field to `Tools` in `src/settings.rs`, defaulting to
   `true`. Add a `--tools` name in `main.rs` `setup()`, and a component in
   `installer/simple-pc-tools.iss`.
4. **Page.** In `ui/index.html`, add a nav link with `data-tool="<tool>"`, a
   `<section id="page-<tool>">`, a `show<Tool>()` function in `route()`, a
   switch on the Settings page, and demo answers in `demoApi`.
5. **Docs.** Add a section to README.md, and to DESIGN.md if you added a
   component.

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
