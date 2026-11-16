# Simple PC Tools design system

The UI is one page shown in the app's own window (WebView2): `ui/index.html`
(the shell and icons), `ui/style.css` (all styles), and one script per page in
`ui/js/`. It should feel like a calm, native Windows 11 settings page: open
it, see at a glance whether anything needs you, change a switch, close it.

`tests/ui_design.rs` enforces the mechanical rules below as part of `cargo test`.
The judgement rules are up to you, so read this before changing the UI.

## Principles

1. **Simple first, power second.** Every tool works with its defaults and a
   switch or two. Anything for developers or power users goes under an
   **Advanced** disclosure, closed by default. A basic user should never need
   to open it.
2. **Status first.** Anything that needs attention is visible without opening
   anything: lamp colour, pill, a red count on the nav item, a tinted left edge.
3. **Calm when fine.** A healthy app is a green lamp and a muted pill. Colour
   and motion are spent only on states that need attention or are in progress.
   Only `starting` and `acting` lamps pulse.
4. **No frills.** No splash screens, tips, upsells, badges for the sake of
   badges, or confirmation dialogs for safe actions. Things happen when you
   click, and a toast says what happened.
5. **Say what happens, in plain words.** Buttons say what they do ("Restart",
   "Scan C:"). Errors say what failed and what to do. Never use internal names
   in the UI: write "Start at sign-in", not `start_at_login`.
6. **Native, offline, fast.** System fonts (Segoe UI Variable, Cascadia Mono).
   No CDNs, frameworks or build step. It must work with no network.
7. **One accent.** Blue (from the icon) is the brand: primary buttons,
   switches, the current nav item, size bars. State colours are separate and
   never used for decoration.

## Themes

- **Light**: clean white. The page and panels are white; the sidebar is a hair
  off-white.
- **Dark**: dark grey, not black. The page is `#202020` and panels are
  `#2b2b2b`, like Windows 11.
- **System** (the default) follows Windows. **Settings → Theme** sets
  `data-theme` on `<html>`.

## Tokens

All tokens are at the top of `ui/style.css`, between `TOKENS:BEGIN` and
`TOKENS:END`. That block is the only place a colour may be written.

| Role | Tokens | Use |
| --- | --- | --- |
| Surfaces | `--bg` `--side` `--surface` `--sunken` `--hover` | page; sidebar; cards and dialogs; wells (chips, activity, inputs); row hover |
| Lines | `--line` `--line-strong` | card borders and dividers; control borders |
| Text | `--fg` `--muted` `--faint` | body; secondary; timestamps, paths, empty states |
| Brand | `--accent` `--accent-hover` `--accent-soft` `--on-accent` | primary button, switches, links, current item |
| State | `--ok` `--warn` `--bad` `--info` `--idle`, each with `-soft` | lamps, pills, chips, banners |
| Data | `--bar` `--bar-track` | size bars, drive meters |
| Effects | `--shadow` `--scrim` `--focus` `--ease` | raised items; dialog backdrop; focus ring |
| Type | `--font-display` `--font-text` `--font-data` | headings; body; paths, sizes, times, JSON |
| Scale | `--step--1` 12, `--step-0` 14, `--step-1` 16, `--step-2` 20, `--step-3` 28 | no other font sizes |
| Space | `--s1`…`--s6` = 4 8 12 16 24 32 | gaps and padding |
| Radius | `--r-sm` 6, `--r-md` 8, `--r-lg` 12, `--r-pill` | controls; wells; cards; pills |

Light values are on the bare `:root`. Dark values are set twice, identically:
under `@media (prefers-color-scheme: dark)`, guarded by
`:root:not([data-theme="light"])`, and under `:root[data-theme="dark"]`. A new
colour token goes in all three places, and the tests check this.

## App health states

| Server state (`Health`) | Label | Colour | Lamp |
| --- | --- | --- | --- |
| `healthy` | Running | ok (green) | steady |
| `starting`, `acting` | Starting / Working… | info (blue) | pulses |
| `failing`, `gave_up` | Not responding / Gave up | bad (red) | steady; tinted left edge; counted in the nav |
| `held` | Left alone | warn (amber) | steady; tinted left edge; counted in the nav |
| `stopped`, `unknown` | Not running / Not checked yet | idle (grey) | steady |

A new state in `src/tools/apps/supervisor.rs` (`Health`) needs a row here, an
entry in `STATES` in `ui/js/apps.js`, a `.st-<state>` rule, and a demo app.

## Components

Reuse these; don't invent look-alikes. A new component gets a line here.

- **Shell**: a sidebar with the brand, the nav (one item per tool, plus
  Settings) and the version. Below 760 px it becomes a top bar, and below
  460 px the nav shows icons only.
- **Page head** `.head`: an `h1`, a one-sentence `.sub`, and `.actions` on the
  right. At most one `.btn.primary` per page.
- **Button** `.btn`, `.btn.primary`, `.btn.ghost`, `.btn.icon` (32 px square,
  with an `aria-label`). `.danger` is only for the armed second click of a
  two-step action.
- **Switch** `.switch` wrapping a real `<input type="checkbox">` with a stable
  `id`. Use it for every on/off setting; never use a bare checkbox.
- **Segmented control** `.seg` with `aria-pressed` buttons, for 2–4 exclusive
  choices (theme).
- **Card** `.card`, holding `.row`s (label, description, control) or `.app`
  rows. `.section` with an `h2` groups cards on the Settings page.
- **Lamp** `.lamp` and **pill** `.pill` inside an element with a `.st-*`
  class. A pill shows only a state label.
- **Probe chip** `.probe.ok` / `.probe.bad`: one per health check, in the data
  font.
- **Banner** `.banner`, `.banner.warn`, `.banner.bad`: page-wide conditions
  only (an update is available, paused, no admin rights, a scan failed).
- **Meter** `.meter > i`: a size or usage bar, with its width set inline as a
  percentage. `.meter.full` turns it red when a drive is over 90 % full.
- **Disclosure** `details.adv`: the **Advanced** sections. Closed unless the
  item already uses advanced settings.
- **Dialog** `dialog.sheet`: the add/edit form, with a header, a scrolling
  body, and a footer whose primary action sits on the right.
- **Breadcrumbs** `.crumbs` and **size table** `.items`: the Disk usage browser.
- **Empty state** `.card.empty`: an icon, a heading, one sentence, and the
  action to take.
- **Toast** `toast(msg)`: confirms every action in one short sentence.
- **Icons**: the inline SVG sprite in `ui/index.html`, `i-<name>`; a tool's nav
  icon is `i-<tool id>`, with a 16 px
  viewBox, `stroke="currentColor"`, 1.6–2 px strokes and round caps. Add new
  ones there. No emoji, icon fonts or external images.

Destructive actions (restart, stop, remove) use the two-step pattern: the first
click turns the button into "Click again to …" for 4 seconds. Never use
`alert`, `confirm` or `prompt`.

## Layout and behaviour

- One centred column, at most 920 px wide. App rows are a three-column grid
  (lamp, details, controls); the controls drop under the details below 760 px.
  The window's minimum size is 560 × 480, and the page must also work at
  about 400 px with no horizontal scrolling.
- The Apps page refreshes every 5 s, and rebuilds the list only when something
  visible has changed, so focus, hover and open activity panels survive. The
  "next check" countdown ticks every second.
- Respect `prefers-reduced-motion`, and keep the `:focus-visible` outlines.
- A new server field shown in the UI also goes into the demo data (`ui/js/demo.js`).
- Run `scripts\screenshots.ps1` after a visible change, so the README pictures stay current.

## The icon

`assets/make_icon.py` draws `assets/app.ico` (embedded in the exe by
`build.rs` as resource 1), `icon-256.png` (the page and README) and
`icon-32.png`. The mark is a blue tile holding a 2 × 2 grid of white tiles,
one of them round: a small set of simple tools. To change it, edit the script
and run `python assets/make_icon.py`; don't edit the `.ico` by hand.

## Checking a change

1. `cargo test --workspace`, which includes these design rules.
2. Run a separate development copy. It isn't elevated, has its own empty data
   folder, and can't touch a real install:

   ```powershell
   cargo run -- --no-elevate --background --dev-server 127.0.0.1:7901 --instance-port 47699 --data-dir target\dev-data
   ```

   This is also the `dev` entry in `.claude/launch.json`. Open
   <http://127.0.0.1:7901/?demo>, which shows every state with sample data and
   never writes anything. Check it in light and dark, at full width and at
   about 420 px. Quit the copy with `--quit --instance-port 47699`.
