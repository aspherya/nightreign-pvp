# Builds dist\NightreignPvP.zip: the drag-and-drop install for players who already have Seamless Co-op.
# Contents (to be dropped into the game folder, next to nightreign.exe and nrsc_launcher.exe):
#   nrpvp_launcher.exe            double-click to play
#   NightreignPvP\nrpvp.dll       the mod
#   NightreignPvP\nrpvp.toml      settings (team size, matchmaking = true (MM-4), mm_max_players = -MatchPlayers (default 2 until release); PvP is always on, no dev_mode)
#   NightreignPvP\gamedata\...    our game-data package (dist\gamedata from scripts\build-gamedata.ps1: Battle Royale texts + Roundtable
#                                 messages, UI-1/UI-2; the hawk crossing line, HAWK-2); the launcher loads it as an me3 [[packages]] entry
#   NightreignPvP\me3\...         bundled me3 0.11.0 (MIT / Apache-2.0), so nothing has to be installed
#   HOW TO INSTALL.txt
# R1: the zip gets a PLAYER nrpvp.dll, built here with `--no-default-features` (no developer tooling) into target\player\. The script
#   also builds the launcher. (R5 removed the -MatchmakingTest developer zip with the debug panel; friend tests use this zip.)
param(
    [string]$Me3Dir = 'Z:\Tools\me3',
    [int]$TeamSize = 1,
    # Pre-release feature flag: players per match in the player zip (2..6). 2 = quick friend tests (default until the first release);
    # 6 = the final game: every match has 6 players (Solo 6 x 1, Duo 3 x 2, Squad 2 x 3). Min = max, so matches only start full. Everyone who should meet needs the same number.
    [ValidateRange(2, 6)][int]$MatchPlayers = 2
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $repo 'dist'
# UI-1/UI-2: the game-data package (our texts in every language + the Roundtable event script). Built by scripts\build-gamedata.ps1
# (needs the unpacked game); the launcher loads NightreignPvP\gamedata as an me3 [[packages]] entry.
$gamedata = Join-Path $dist 'gamedata'
$gdFiles = @(Get-ChildItem -LiteralPath $gamedata -Recurse -File -ErrorAction SilentlyContinue)
# HAWK-2: the crossing line (map\mapstudio\m60_00_00_99.msb.dcx) is part of the package; it is the only file allowed under map\.
$routes = Join-Path $gamedata 'map\mapstudio\m60_00_00_99.msb.dcx'
$gdMap = @($gdFiles | Where-Object { $_.FullName -like (Join-Path $gamedata 'map\*') })
if ($gdMap.Count -ne 1 -or -not (Test-Path -LiteralPath $routes)) {
    throw "dist\gamedata\map must hold exactly map\mapstudio\m60_00_00_99.msb.dcx (the hawk crossing line; found $($gdMap.Count) file(s)). Run scripts\build-gamedata.ps1 (without -VanillaRoutes) first."
}
$gdMsg = @($gdFiles | Where-Object { $_.Name -eq 'menu_dlc01.msgbnd.dcx' })
# OFF-4: event\common.emevd.dcx holds the bot events (dev only: left out of the zip below).
if (-not (Test-Path -LiteralPath (Join-Path $gamedata 'event\m10_00_00_00.emevd.dcx')) -or -not (Test-Path -LiteralPath (Join-Path $gamedata 'event\common.emevd.dcx')) -or $gdMsg.Count -ne 15 -or $gdFiles.Count -ne 18) {
    throw "dist\gamedata incomplete ($($gdFiles.Count) files, $($gdMsg.Count) languages; want event\m10_00_00_00.emevd.dcx + event\common.emevd.dcx + 15 x msg\<lang>\menu_dlc01.msgbnd.dcx + the hawk route file). Run scripts\build-gamedata.ps1 first."
}
foreach ($f in 'bin\me3.exe', 'bin\me3-launcher.exe', 'bin\me3_mod_host.dll', 'LICENSE-MIT', 'LICENSE-APACHE') {
    if (-not (Test-Path -LiteralPath (Join-Path $Me3Dir $f))) { throw "me3 file missing: $Me3Dir\$f" }
}

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
$prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'   # cargo writes progress to stderr
& $cargo build --release -p nrpvp-launcher --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1 | Out-Null
$code = $LASTEXITCODE; $ErrorActionPreference = $prev
if ($code -ne 0) { throw "cargo build of the launcher failed ($code)" }
$playerTarget = Join-Path $repo 'target\player'
$prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
& $cargo build --release -p nrpvp --no-default-features --target-dir $playerTarget --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1 | Out-Null
$code = $LASTEXITCODE; $ErrorActionPreference = $prev
if ($code -ne 0) { throw "cargo build of the player nrpvp.dll failed ($code)" }
$dll = Join-Path $playerTarget 'release\nrpvp.dll'
$launcher = Join-Path $repo 'target\release\nrpvp_launcher.exe'

$stage = Join-Path $dist 'release-stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
$mod = Join-Path $stage 'NightreignPvP'
$me3 = Join-Path $mod 'me3'
New-Item -ItemType Directory -Force $me3 | Out-Null

Copy-Item -LiteralPath $launcher -Destination $stage
Copy-Item -LiteralPath $dll -Destination $mod
Copy-Item -LiteralPath $gamedata -Destination (Join-Path $mod 'gamedata') -Recurse
# OFF-4/OFF-5: the bot events (event\common.emevd.dcx) are an internal development tool for now: players never get them.
Remove-Item -LiteralPath (Join-Path $mod 'gamedata\event\common.emevd.dcx') -Force
Copy-Item -LiteralPath (Join-Path $Me3Dir 'bin\me3.exe'), (Join-Path $Me3Dir 'bin\me3-launcher.exe'), (Join-Path $Me3Dir 'bin\me3_mod_host.dll') -Destination $me3
Copy-Item -LiteralPath (Join-Path $Me3Dir 'LICENSE-MIT'), (Join-Path $Me3Dir 'LICENSE-APACHE') -Destination $me3
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'release\HOW TO INSTALL.txt') -Destination $stage

$ascii = New-Object Text.ASCIIEncoding
$toml = @(
    '# Nightreign PvP settings. You do not need to change anything here.',
    "team_size = $TeamSize",
    '# Solo Battle Royale matchmaking: table > Matchmaking Settings > Number of Players = Solo > Commence.',
    '# Set to false to turn it off.',
    'matchmaking = true',
    '# Players per match (2..6). Pre-release builds use 2 so two friends can test; the final game uses 6. Everyone needs the same number.',
    "mm_max_players = $MatchPlayers",
    "mm_min_players = $MatchPlayers",
    'team_candidates = [29, 6, 3, 16, 13, 2, 1]',
    '',
    '# Hawk crossing: the start hawk flies one line across the map. Hold the stick to dive, then steer; leave the hawk with the',
    '# Spectral Hawk Tree buttons (hold action, or L3). Keep this table last.',
    '[hawk]',
    'enabled = true'
) -join "`r`n"
[IO.File]::WriteAllText((Join-Path $mod 'nrpvp.toml'), $toml + "`r`n", $ascii)

$commit = (& git -C $repo rev-parse --short HEAD 2>$null)
$dirty = if (& git -C $repo status --porcelain 2>$null) { 'dirty' } else { 'clean' }
$gdHash = (Get-FileHash (Join-Path $gamedata 'event\m10_00_00_00.emevd.dcx') -Algorithm SHA256).Hash.Substring(0,16)
$routeHash = (Get-FileHash $routes -Algorithm SHA256).Hash.Substring(0,16)
$ver = "built $((Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm')) UTC, commit $commit ($dirty), nrpvp.dll $((Get-FileHash $dll -Algorithm SHA256).Hash.Substring(0,16)), gamedata emevd $gdHash, hawk routes $routeHash, me3 0.11.0"
[IO.File]::WriteAllText((Join-Path $mod 'VERSION.txt'), $ver + "`r`n", $ascii)

$zipName = 'NightreignPvP.zip'
$zip = Join-Path $dist $zipName
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
Write-Output "OK: $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
Write-Output $ver
