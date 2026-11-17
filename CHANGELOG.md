# Changelog

## 0.2.0 (2026-11-17)

- **Fixed: apps could vanish after a restart.** When Simple PC Tools (or its installer) was started from inside a packaged app such as Claude Desktop or some Microsoft Store apps, Windows quietly saved its app list in that app's private folder. The copy that starts with Windows couldn't see it, so after a restart the Apps page was empty. Now:
  - a copy started inside such a sandbox hands over to the sign-in task, which runs outside it (or, without the task, says to open it from the Start menu);
  - the installer leaves its tool choices where the app picks them up;
  - an app list already stranded in a package's folder is brought back automatically the first time the app starts with an empty list.
- New tool: **Resources**. See what's using your memory, processor and network, grouped by program (30 browser processes are one row).
  - A stacked memory chart of the last 15 minutes. Point at it to see which program each layer is, and how much it used at that moment.
  - Search filters as you type: by name, exe, folder, process ID or port, with the matches highlighted. Type a port number to see which program has it open. Keys: `/` or Ctrl+F to search, Enter opens the top result, arrows move through the list, Esc clears.
  - Open a program to see its file, its open ports and its processes, show it in File Explorer, or end it (two clicks; parts of Windows can't be ended here).
  - Under **Advanced**: committed memory, process and thread counts, uptime, and every network port with its program.
  - It samples every second while the page is open, and every 5 seconds otherwise to keep the chart's history.
- The exe now carries its name and version, so Task Manager shows "Simple PC Tools".
- The installer's tool list comes from one `ToolIds` line, and CI checks a tool left unticked stays off.

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
