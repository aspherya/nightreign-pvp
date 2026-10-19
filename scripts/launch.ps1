# Launch ELDEN RING NIGHTREIGN through me3 (no EAC, official servers blocked).
# Run ./scripts/build.ps1 first. Never go online with a modded game.
#
#   ./scripts/launch.ps1                 # -Profile sc (default): Seamless Co-op + nrpvp.dll
#   ./scripts/launch.ps1 -Profile dev    # nrpvp.dll only, no SC
#   ./scripts/launch.ps1 -Profile br     # SC + nrpvp.dll + our game-data package (run ./scripts/build-gamedata.ps1 first)
#   ./scripts/launch.ps1 -Profile ui0a   # UI-0 step 6a: SC + nrpvp.dll + recompiled table script (dist\ui0-pkg-a)
#   ./scripts/launch.ps1 -Profile ui0b   # UI-0 step 6b: SC + nrpvp.dll + "Battle Royale (test)" list at the table (dist\ui0-pkg-b)
#   ./scripts/launch.ps1 -DisableArxan   # troubleshooting: adds me3's --disable-arxan
#
# CLI used: `me3 launch -g nightreign -p <profile> [--disable-arxan]` (from `me3 launch --help`, me3 0.11.0).
# Profiles: profiles\nrpvp-sc.me3.toml (sc) and profiles\nrpvp-dev.me3.toml (dev).
[CmdletBinding()]
param(
    [ValidateSet('sc', 'br', 'dev', 'ui0a', 'ui0b')]
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
