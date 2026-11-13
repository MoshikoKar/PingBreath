# NetPulse

![License](https://img.shields.io/badge/license-MIT-blue.svg)

Windows tray app that monitors network latency to your router and chosen hosts,
with live sparklines, 24-hour health history, outage notifications, and
checksum-verified self-update from GitHub Releases.

## Description

NetPulse sits in the system tray and opens a compact always-on-top panel that
pings configurable targets (default gateway, plus hosts such as Google DNS and
Cloudflare). It shows per-target latency, loss, jitter, and a rolling daily
summary so you can spot Wi-Fi or ISP issues quickly. It has no taskbar button;
the tray icon is the only way in and the only way to quit.

Settings and history live under `%APPDATA%\NetPulse` by default. A portable
build keeps them in `data\` next to the executable when `portable.txt` is present.

## Installation

### Prebuilt release

Download from the [latest release](https://github.com/MoshikoKar/NetPulse/releases/latest):

- `NetPulse-Setup.exe` - per-user installer (no admin rights), optional start with Windows
- `NetPulse-portable.zip` - unzip anywhere and run; settings stay beside the exe

Each release lists the SHA-256 of both files (also in `SHA256SUMS.txt`). Verify with
`certutil -hashfile NetPulse-Setup.exe SHA256`.

> **SmartScreen:** the binaries are not code-signed yet, so Windows may show
> "Windows protected your PC". Choose *More info -> Run anyway* after checking the hash.

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

Release packaging (exe, portable zip, setup if Inno Setup is installed, checksums):

```powershell
.\build.ps1
# or: .\build.ps1 -SkipSetup
```

Artifacts are written to `dist/`. Close a running NetPulse first (tray icon -> Quit),
otherwise the build cannot overwrite the exe.

## Usage

The app starts in the tray. Left-click the tray icon to show or hide the panel.
The **X** on the panel only hides it. Right-click either the tray icon or the panel for:

- Always on top, Lock position, Start with Windows (toggles, remembered)
- Edit destinations... (opens the config in Notepad), Reload config
- Check for updates
- Quit (tray icon menu only)

### Configuration

`%APPDATA%\NetPulse\config.json` (or `data\config.json` in portable mode). Missing keys use defaults.

```json
{
  "targets": [
    { "name": "Router", "host": "gateway" },
    { "name": "Google", "host": "8.8.8.8", "warn_ms": 60, "bad_ms": 150 },
    { "name": "Cloudflare", "host": "1.1.1.1" }
  ],
  "interval_ms": 1000,
  "timeout_ms": 2000,
  "always_on_top": true,
  "locked": false,
  "check_updates": true,
  "update_channel": "stable",
  "notifications": true
}
```

| Key | Meaning |
|---|---|
| `targets` | Up to 8 destinations. **The router (`"host": "gateway"`, auto-detected) is mandatory and always first**; if you remove it, it is re-added. Everything else is yours to change. |
| `warn_ms` / `bad_ms` | Optional per target; latency at which it turns amber / red (default 80 / 200). |
| `interval_ms`, `timeout_ms` | Ping cadence and timeout. |
| `always_on_top`, `locked` | Same as the menu toggles. |
| `check_updates` | Look for a new release shortly after startup (prompts only if one exists). |
| `update_channel` | `stable` or `beta` (also offers pre-releases). |
| `notifications` | Windows toast when a target goes down and when it recovers. |

Use *Reload config* in the menu after editing.

### Updates

*Check for updates* (and the automatic check) read the latest GitHub release. Before
anything is run, the download is checked against that release's `SHA256SUMS.txt`; a
missing or mismatching checksum aborts the update. Installed copies run the new setup
silently; portable copies swap the exe and keep the old one as `NetPulse.exe.bak`.

## Features

- System-tray UI, always-on-top and lock, no taskbar button, single instance
- ICMP latency monitoring via Windows IP Helper
- Live sparkline, avg / jitter / loss, and 24-hour hourly health
- Up to 8 targets with per-target thresholds; router target always first
- Outage / recovery toast notifications
- Start with Windows toggle
- Portable mode via `portable.txt` next to the exe
- Checksum-verified GitHub Releases self-update with stable / beta channels

## Releasing (maintainers)

1. Add a `## [x.y.z] - date` section to `CHANGELOG.md`.
2. `.\release.ps1 -Bump patch` (or `minor` / `major`, or `-Version x.y.z`, or `x.y.z-beta.1` for a pre-release).
3. The tag triggers `.github/workflows/release.yml`, which builds and publishes the setup, the
   portable zip and `SHA256SUMS.txt`, with the changelog section and hashes as the release notes.

CI (`.github/workflows/ci.yml`) runs `cargo test` and `cargo clippy` on every push and pull request.

## Tech Stack

- Rust 2024 edition (`Cargo.toml`)
- [Slint](https://slint.dev/) UI
- `tray-icon` for the system tray
- Windows crates (`windows`, Win32 ICMP / window APIs)
- Inno Setup installer (`installer/NetPulse.iss`)
- GitHub Actions (CI + release)

## Contributing

Pull requests are welcome. For substantial changes, open an issue first to
discuss the approach.

## License

MIT - see the `LICENSE` file for details.
