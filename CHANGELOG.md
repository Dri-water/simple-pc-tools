# Changelog

## 0.1.1 (2026-11-16)

- Installer: the full standard wizard (Welcome, Licence, Install location, Select components, Additional tasks, Ready, Finish), with every tool and "Start when I sign in" ticked by default. Uninstalling asks whether to keep your settings.
- Tools are registered in one list (`src/tools/mod.rs`), and the UI is split into one script per page, so adding a tool is a short, test-checked recipe (CONTRIBUTING.md).
- README screenshots and an installer walkthrough.
- CI builds the installer and tests a real install and uninstall on every push.

## 0.1.0 (2026-11-16)

First release.

- **Apps**: start programs at sign-in and keep them running. Advanced: URL, port, file and process checks; hold conditions; restart limits; custom start/stop steps; run as administrator.
- **Disk usage**: folders sorted by size, read straight from the NTFS file table when possible.
- Installer with tool choice and Start with Windows; daily update check with Update / Not now / Skip this version.
