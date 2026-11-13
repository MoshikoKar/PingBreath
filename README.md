# NetPulse

![License](https://img.shields.io/badge/license-MIT-blue.svg)

Windows tray app that monitors network latency to your router and chosen hosts,
with live sparklines, 24-hour health history, and optional self-update from
GitHub Releases.

## Description

NetPulse sits in the system tray and opens a compact always-on-top panel that
pings configurable targets (default gateway, plus hosts such as Google DNS and
Cloudflare). It shows per-target latency, loss, jitter, and a rolling daily
summary so you can spot Wi-Fi or ISP issues quickly.

Settings and history live under `%APPDATA%\NetPulse` by default. A portable
build keeps them next to the executable when `portable.txt` is present.

## Installation

### Prebuilt release

Download `NetPulse-Setup.exe` or `NetPulse-portable.zip` from the
[latest release](https://github.com/MoshikoKar/NetPulse/releases/latest).

### Build from source

Requirements:

- Rust toolchain (stable)
- Windows (Win32 ICMP / tray APIs)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) only if you want the installer

```powershell
git clone https://github.com/MoshikoKar/NetPulse.git
cd NetPulse
cargo build --release
```

Release packaging (exe, portable zip, and setup if Inno Setup is installed):

```powershell
.\build.ps1
# or: .\build.ps1 -SkipSetup
```

Artifacts are written to `dist/`.

## Usage

Run the release binary:

```powershell
.\target\release\netpulse.exe
```

Or after packaging:

```powershell
.\dist\NetPulse.exe
```

The app starts in the tray. Click the tray icon to show or hide the panel.
Right-click the tray icon for pin, lock, update check, and quit.

Default targets:

- Router (auto-detected default gateway)
- Google (`8.8.8.8`)
- Cloudflare (`1.1.1.1`)

Config path (installed / normal mode): `%APPDATA%\NetPulse\config.json`

Tagged releases (`v*`) trigger `.github/workflows/release.yml`, which publishes
`NetPulse-Setup.exe` and `NetPulse-portable.zip` for the in-app updater.

## Features

- System-tray UI with optional always-on-top and lock
- ICMP latency monitoring via Windows IP Helper
- Live sparkline, avg / jitter / loss, and 24-hour hourly health
- Up to 8 targets; router/`gateway` target is always first
- Portable mode via `portable.txt` next to the exe
- Optional GitHub Releases self-update

## Tech Stack

- Rust 2024 edition (`Cargo.toml`)
- [Slint](https://slint.dev/) UI
- `tray-icon` for the system tray
- Windows crates (`windows`, Win32 ICMP / window APIs)
- Inno Setup installer (`installer/NetPulse.iss`)
- GitHub Actions release workflow

## Contributing

Pull requests are welcome. For substantial changes, open an issue first to
discuss the approach.

## License

MIT - see the `LICENSE` file for details.
