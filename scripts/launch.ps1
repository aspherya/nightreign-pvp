# Launch ELDEN RING NIGHTREIGN through me3 with the dev profile (no EAC, official servers blocked).
# Run ./scripts/build.ps1 first. Never go online with a modded game.
# CLI used: `me3 launch -g nightreign -p <profile>` (from `me3 launch --help`, me3 0.11.0).
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$profilePath = Join-Path $repo 'profiles\nrpvp-dev.me3.toml'
$dll = Join-Path $repo 'dist\nrpvp.dll'

if (-not (Test-Path $dll)) { throw "dist\nrpvp.dll not found. Run ./scripts/build.ps1 first." }
if (-not (Test-Path $profilePath)) { throw "Profile not found: $profilePath" }

$me3 = (Get-Command me3 -ErrorAction SilentlyContinue).Source
if (-not $me3) {
    $candidate = 'Z:\Tools\me3\bin\me3.exe'
    if (Test-Path $candidate) { $me3 = $candidate }
}
if (-not $me3) { throw 'me3 not found on PATH or at Z:\Tools\me3\bin\me3.exe.' }

Write-Host "Launching via me3: $profilePath"
& $me3 launch -g nightreign -p $profilePath
exit $LASTEXITCODE
