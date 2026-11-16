# Building Simple PC Tools yourself

If you'd rather not run a downloaded `.exe`, build it from source. It takes
about five minutes the first time.

## What you need

- **Windows 10 or 11**, 64-bit.
- **Rust**: install it from <https://rustup.rs>, and accept the default
  options. When rustup offers to install the Visual Studio C++ Build Tools,
  say yes: Rust needs them on Windows.
- **Git**, to download the source: <https://git-scm.com/download/win>. You can
  also use **Code → Download ZIP** on GitHub.
- The **WebView2 runtime**, which shows the app's window. Windows 10 and 11
  already include it.

## Build

Open **PowerShell** and run:

```powershell
git clone https://github.com/Dri-water/simple-pc-tools.git
cd simple-pc-tools
cargo build --release
```

The app is now at `target\release\simple-pc-tools.exe`. It's a single file:
copy it anywhere and run it.

## Install what you built

**Option A: just run it.** Double-click `simple-pc-tools.exe`. It asks for
administrator rights once. To start it with Windows, turn on **Settings →
Start with Windows** in the app. That creates the same sign-in task the
installer does.

**Option B: make the installer too.** Install [Inno Setup 6](https://jrsoftware.org/isdl.php),
then run:

```powershell
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" /DAppVersion=0.1.0 installer\simple-pc-tools.iss
```

The installer is written to `target\installer\`.

## Check it works

```powershell
cargo test --workspace
```

This runs the unit tests and the UI design rules. One fast-scan test needs an
administrator PowerShell, so it is skipped by default. To run it:

```powershell
cargo test -p disk-scan -- --ignored
cargo run --release -p disk-scan --example scan -- C:\
```

## Check that a release matches the source

Each release is built by [the release workflow](.github/workflows/release.yml)
from the tagged commit, and lists SHA-256 checksums in `SHA256SUMS.txt`. To
check a download:

```powershell
Get-FileHash .\simple-pc-tools-setup-0.1.0.exe -Algorithm SHA256
```

Rust builds aren't byte-for-byte reproducible on every machine, so your own
build's checksum may differ from the release's even when the code is the same.
