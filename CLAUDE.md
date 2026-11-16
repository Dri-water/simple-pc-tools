# simple-pc-tools

An open-source (GPL-3.0) Windows app of small tools: **Apps** (start programs at
sign-in and keep them running) and **Disk usage** (fast folder sizes). It runs
as administrator from a Task Scheduler sign-in task, with a tray icon and a
WebView2 window. Read CONTRIBUTING.md for the layout.

## Rules

- **Simple first.** Defaults must work for non-technical users. Power-user
  options go under an "Advanced" disclosure. Plain words in the UI, never
  internal field names.
- **UI changes follow DESIGN.md.** Colours only as tokens in the
  `TOKENS:BEGIN`…`TOKENS:END` block, with the light value and both dark
  blocks. Font sizes and families only via `--step-*` / `--font-*`. Reuse the
  listed components, and document new ones. No external resources, no
  frameworks or build step, no `alert`/`confirm`/`prompt`. A new server field
  shown in the UI also gets demo data in `demoApi`.
- **Security.** The app is elevated. Its API is served only to its own window
  over the `spt://` custom protocol. Don't add network listeners. Apps the user
  adds start with their normal rights unless they choose "Run as
  administrator" (`src/tools/apps/launch.rs`).
- **Privacy.** No telemetry. The only network call is the daily GitHub release
  check (`src/update.rs`).

## Done means

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` all pass. The tests include the UI design rules
  in `tests/ui_design.rs`.
- For UI changes, `/?demo` has been looked at on a development copy in light
  and dark, wide and at about 420 px (DESIGN.md, "Checking a change").

## Never

- Test against a real install. Always use a development copy:
  `--no-elevate --dev-server 127.0.0.1:7901 --instance-port 47699 --data-dir target\dev-data`.
  A copy on the default instance port or data folder would start and restart
  real programs.
- Change the `AppId` in `installer/simple-pc-tools.iss`, or the task name in
  `src/autostart.rs`: installed copies depend on them.
