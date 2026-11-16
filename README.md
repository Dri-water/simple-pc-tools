<p align="center"><img src="assets/icon-256.png" width="96" alt=""></p>

<h1 align="center">Simple PC Tools</h1>

<p align="center">Small tools for your Windows PC that do what you want, with no frills.<br>
Free and open source. No ads, no accounts, no tracking.</p>

---

There is a lot of annoying software out there: bloated, nagging, full of
upsells. Simple PC Tools is a growing set of small, plain tools that each do
one job well. Anyone can use them without reading a manual, and developers and
power users get an **Advanced** section when they want more control.

We build what people actually ask for. If there is a simple tool you wish you
had, or something here bugs you, [open an issue](../../issues/new/choose).

## The tools

### Apps: start programs when you sign in, and keep them running

Add a program, and Simple PC Tools starts it when you sign in. With
**Keep running** on, it restarts the program if it closes or crashes.

Under **Advanced**, power users can also:

- check that a URL answers, a port is open or a file exists, not just that the process runs;
- watch a different process when the program is a launcher or script;
- leave an app alone while a condition holds (for example, while you've paused it on purpose);
- set restart limits, a grace period after starting, and custom start and stop steps;
- run an app as administrator. By default apps start with your normal rights, just like opening them from the Start menu.

Programs it starts keep running if Simple PC Tools is closed or updated.

### Disk usage: see what fills your drive

Pick a drive or folder and see its folders sorted by size, biggest first. Click
into any folder, or open it in File Explorer. Because the app runs as
administrator, it reads the drive's file table directly (like WizTree), which
is much faster than going through folders one by one. It only looks: it never
changes or deletes anything.

### More to come

Tell us what you'd like next.

## Install

1. Download `simple-pc-tools-setup-<version>.exe` from the
   [latest release](../../releases/latest).
2. Run it. Choose your tools (all are selected by default) and whether to
   start with Windows (on by default).
3. Find Simple PC Tools in the Start menu and in the tray by the clock.

There is also a portable zip with just the `.exe` if you don't want an installer.

> [!WARNING]
> **The installer isn't code-signed yet, so Windows will warn you.** Signing
> certificates cost money every year, and we haven't got one yet. You'll see:
>
> - **Your browser** may say the file "isn't commonly downloaded". Choose **Keep** (in Edge, under **…**).
> - **Windows SmartScreen** says "Windows protected your PC". Click **More info**, then **Run anyway**.
>
> If you'd rather not trust a download, every release is built from this
> source code by [GitHub Actions](.github/workflows/release.yml), and each
> release lists SHA-256 checksums. You can also [build it yourself](BUILD.md).

### What the installer sets up

- The app, in `C:\Program Files\Simple PC Tools`.
- A Start menu shortcut, and a desktop shortcut if you ask for one.
- A Task Scheduler task, **Simple PC Tools**, that starts it when you sign
  in, **with administrator rights and without a prompt**. Turn it on or off
  any time in **Settings → Start with Windows**.

Settings and logs live in `%LOCALAPPDATA%\Simple PC Tools`. Uninstall it from
**Settings → Apps** in Windows like any other app. Uninstalling removes the
sign-in task, and the apps it started keep running.

### Why administrator rights?

The fast disk scan needs them, and so does starting at sign-in without a UAC
prompt. Apps you add still start with your **normal** rights unless you choose
**Run as administrator** for that app. The app's controls are only reachable
from its own window, never over the network.

## Updates

About once a day, the app asks GitHub whether there's a newer release. Only
that request is sent: nothing about you or your PC. When there is one, you can
choose:

- **Update**: downloads the installer, checks its SHA-256 checksum against the
  one GitHub publishes, and installs it. The app restarts on its own.
- **Not now**: asks again in a few days.
- **Skip this version**: never asks about that version again.

To stop the checks entirely, turn off **Settings → Check for updates**.

## Contributing

Ideas, bug reports and code are all welcome. See [CONTRIBUTING.md](CONTRIBUTING.md)
for how the project is put together and how to add a tool, and
[DESIGN.md](DESIGN.md) for the UI rules that keep it looking consistent.

## Licence

[GPL-3.0-or-later](LICENSE). You can use, study, change and share it. If you
distribute a changed version, it must stay open source under the same licence.
