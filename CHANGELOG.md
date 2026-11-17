# Changelog

All notable changes. Format: `## [version] - date`. The release workflow publishes the section
matching the tag as the release notes, so add the section **before** running `release.ps1`.

## [0.3.0] - 2026-11-17

### Changed
- Renamed the app from NetPulse to PingBreath (package, UI, installer, release assets, docs).
- Settings now live under `%APPDATA%\PingBreath`. On first start, `config.json` and `history.json`
  are copied from `%APPDATA%\NetPulse` when present (the old folder is left untouched).
- "Start with Windows" writes the `PingBreath` Run value and removes a leftover `NetPulse` entry.
- Self-updater looks at `MoshikoKar/PingBreath` and the `PingBreath-*` assets; portable updates
  write `PingBreath.exe` (and drop a leftover `NetPulse.exe` when upgrading an old portable layout).
- Single-instance mutex is `Local\PingBreath.SingleInstance` (also claims the old mutex so both
  builds cannot run at once).

### Notes
- v0.3.0 also publishes legacy `NetPulse-Setup.exe` / `NetPulse-portable.zip` assets so existing
  v0.1/v0.2 clients can self-update. Those bridge assets and the in-app migration shims are
  scheduled for removal in v0.3.1.

## [0.2.0] - 2026-11-14

### Added
- Updater verifies every download against the release's `SHA256SUMS.txt` before running it
  (refuses to update if the checksum is missing or does not match).
- Single instance: a second launch exits instead of adding another tray icon.
- Saved window position is ignored if it is no longer on a connected monitor.
- "Start with Windows" toggle in the tray and window menus (shares the installer's option).
- Windows toast when a target goes down and when it recovers (debounced; `notifications` in config).
- Per-target `warn_ms` / `bad_ms` thresholds in the config (defaults 80 / 200 ms).
- Update channel: `update_channel` = `stable` (default) or `beta` (also offers pre-releases).
- Portable updates keep the previous exe as `NetPulse.exe.bak`.
- Unit tests and a CI workflow (`cargo test`, `cargo clippy`).

### Changed
- Release notes now come from this changelog.

## [0.1.0] - 2026-11-13

- First release: tray latency monitor with live sparklines and 24 h history, always-on-top and lock,
  editable destinations (Router mandatory), portable and setup builds, update check from GitHub.
