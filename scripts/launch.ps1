# Launch ELDEN RING NIGHTREIGN through me3 (no EAC, official servers blocked).
# Run ./scripts/build.ps1 first. Never go online with a modded game.
#
#   ./scripts/launch.ps1                 # -Profile sc (default): Seamless Co-op + nrpvp.dll + our game-data package (dist\gamedata)
#   ./scripts/launch.ps1 -Profile dev    # nrpvp.dll only, no SC, no package
#   ./scripts/launch.ps1 -DisableArxan   # troubleshooting: adds me3's --disable-arxan
#
# CLI used: `me3 launch -g nightreign -p <profile> [--disable-arxan]` (from `me3 launch --help`, me3 0.11.0).
# Profiles: profiles\nrpvp-sc.me3.toml (sc) and profiles\nrpvp-dev.me3.toml (dev).
[CmdletBinding()]
param(
    [ValidateSet('sc', 'dev')]
    [string]$Profile = 'sc',

    # Troubleshooting only. Not needed by default: see the comments in profiles\nrpvp-sc.me3.toml.
    [switch]$DisableArxan
)
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$profilePath = Join-Path $repo "profiles\nrpvp-$Profile.me3.toml"
$dll = Join-Path $repo 'dist\nrpvp.dll'
$scDll = 'Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\SeamlessCoop\nrsc.dll'

if (-not (Test-Path $dll)) { throw "dist\nrpvp.dll not found. Run ./scripts/build.ps1 first." }
if (-not (Test-Path $profilePath)) { throw "Profile not found: $profilePath" }
if ($Profile -ne 'dev' -and -not (Test-Path $scDll)) { throw "Seamless Co-op not found at $scDll (profile '$Profile' needs it)." }
# UI-1: the sc profile loads our game-data package; build it once if it is missing (needs the unpacked game, see build-gamedata.ps1).
if ($Profile -eq 'sc' -and -not (Test-Path (Join-Path $repo 'dist\gamedata\msg'))) {
    Write-Host 'dist\gamedata missing: building it (scripts\build-gamedata.ps1)...'
    & (Join-Path $PSScriptRoot 'build-gamedata.ps1')
}

$me3 = (Get-Command me3 -ErrorAction SilentlyContinue).Source
if (-not $me3) {
    $candidate = 'Z:\Tools\me3\bin\me3.exe'
    if (Test-Path $candidate) { $me3 = $candidate }
}
if (-not $me3) { throw 'me3 not found on PATH or at Z:\Tools\me3\bin\me3.exe.' }

$me3Args = @('launch', '-g', 'nightreign', '-p', $profilePath)
if ($DisableArxan) { $me3Args += '--disable-arxan' }

Write-Host "Launching via me3: $me3 $($me3Args -join ' ')"
& $me3 @me3Args
exit $LASTEXITCODE
