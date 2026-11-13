<#
  Cut a release: bump Cargo.toml, commit, tag vX.Y.Z, push. GitHub Actions
  (.github/workflows/release.yml) then builds and publishes NetPulse-Setup.exe +
  NetPulse-portable.zip, and every installed client sees it via "Check for updates".

  .\release.ps1 -Bump patch      0.1.0 -> 0.1.1   (also: minor, major)
  .\release.ps1 -Version 0.2.0   explicit version
  .\release.ps1 -Version 0.1.0   tag the current version as-is (first release)
  .\release.ps1 -Version 0.3.0-beta.1   pre-release (only clients on update_channel "beta" see it)

  Add the new version's section to CHANGELOG.md first; the release notes are taken from it.
#>
param([ValidateSet('patch','minor','major')][string]$Bump, [string]$Version)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (git status --porcelain) { throw 'Working tree not clean - commit or stash first.' }
$toml = Get-Content Cargo.toml -Raw
$cur = [regex]::Match($toml, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value

if (-not $Version) {
    if (-not $Bump) { throw 'Pass -Bump patch|minor|major or -Version x.y.z' }
    $p = ($cur -split '-')[0].Split('.') | ForEach-Object { [int]$_ }
    switch ($Bump) {
        'major' { $p = @(($p[0]+1), 0, 0) }
        'minor' { $p = @($p[0], ($p[1]+1), 0) }
        'patch' { $p = @($p[0], $p[1], ($p[2]+1)) }
    }
    $Version = $p -join '.'
}
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$') { throw "Bad version '$Version'" }
if (git tag -l "v$Version") { throw "Tag v$Version already exists" }
if (-not (Select-String -Path CHANGELOG.md -Pattern "^## \[$([regex]::Escape($Version))\]" -Quiet)) { throw "CHANGELOG.md has no '## [$Version]' section" }

if ($Version -ne $cur) {
    $toml = [regex]::Replace($toml, '(?m)^version\s*=\s*"[^"]+"', "version = `"$Version`"", 1)
    Set-Content Cargo.toml $toml -NoNewline
    cargo check --release        # refreshes Cargo.lock
    if ($LASTEXITCODE) { throw 'cargo check failed' }
    git add Cargo.toml Cargo.lock
    git commit -m "Release v$Version"
}
git tag "v$Version"
git push origin HEAD "v$Version"
Write-Host "Pushed v$Version - watch the build: gh run watch"
