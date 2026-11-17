<#
  Builds PingBreath and packages it into dist/:
    PingBreath.exe            raw release exe
    PingBreath-portable.zip   PingBreath.exe + portable.txt (config/history kept in .\data)
    PingBreath-Setup.exe      installer (needs Inno Setup 6; skipped with a warning if missing)
    # LEGACY(NetPulse): remove in v0.3.1 - also emit NetPulse-Setup.exe and NetPulse-portable.zip
    # (zip member NetPulse.exe) so v0.1/v0.2 clients can still self-update.
  Usage:  .\build.ps1 [-SkipSetup]
#>
param([switch]$SkipSetup)
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
Write-Host "Building PingBreath $version"

cargo build --release
if ($LASTEXITCODE) { throw 'cargo build failed' }

$dist = Join-Path $PSScriptRoot 'dist'
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $dist | Out-Null
$tgt = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $PSScriptRoot 'target' }
Copy-Item (Join-Path $tgt 'release\pingbreath.exe') (Join-Path $dist 'PingBreath.exe')

# Portable zip
$stage = Join-Path $dist '_portable'
New-Item -ItemType Directory $stage | Out-Null
Copy-Item (Join-Path $dist 'PingBreath.exe') $stage
Set-Content -Path (Join-Path $stage 'portable.txt') -Value 'Delete this file to store settings in %APPDATA%\PingBreath instead.'
Compress-Archive -Path "$stage\*" -DestinationPath (Join-Path $dist 'PingBreath-portable.zip')
Remove-Item $stage -Recurse -Force

# LEGACY(NetPulse): remove in v0.3.1
# Same binary, named NetPulse.exe so the old portable updater can swap the file in place.
# On next start, migrate_legacy_exe_name() renames to PingBreath.exe.
$legacyStage = Join-Path $dist '_legacy_portable'
New-Item -ItemType Directory $legacyStage | Out-Null
Copy-Item (Join-Path $dist 'PingBreath.exe') (Join-Path $legacyStage 'NetPulse.exe')
Set-Content -Path (Join-Path $legacyStage 'portable.txt') -Value 'Delete this file to store settings in %APPDATA%\PingBreath instead.'
Compress-Archive -Path "$legacyStage\*" -DestinationPath (Join-Path $dist 'NetPulse-portable.zip')
Remove-Item $legacyStage -Recurse -Force

# Installer
if (-not $SkipSetup) {
    $iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
            Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if ($iscc) {
        & $iscc "/DAppVersion=$version" installer\PingBreath.iss
        if ($LASTEXITCODE) { throw 'Inno Setup failed' }
        # LEGACY(NetPulse): remove in v0.3.1 - duplicate setup under the old asset name for v0.1/v0.2 updaters.
        Copy-Item (Join-Path $dist 'PingBreath-Setup.exe') (Join-Path $dist 'NetPulse-Setup.exe')
    } else {
        Write-Warning 'Inno Setup 6 not found - skipped PingBreath-Setup.exe (winget install JRSoftware.InnoSetup)'
    }
}

# SHA-256 checksums for the release files (new + legacy names)
$wanted = @('PingBreath-Setup.exe','PingBreath-portable.zip','NetPulse-Setup.exe','NetPulse-portable.zip') # LEGACY(NetPulse): remove in v0.3.1 - drop NetPulse-* from this list
$sums = Get-ChildItem $dist -File | Where-Object { $_.Name -in $wanted } |
    ForEach-Object { '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name }
Set-Content -Path (Join-Path $dist 'SHA256SUMS.txt') -Value $sums
$sums | ForEach-Object { Write-Host $_ }

Get-ChildItem $dist | Format-Table Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}}
