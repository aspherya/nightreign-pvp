# Builds dist\NightreignPvP.zip: the drag-and-drop install for players who already have Seamless Co-op.
# Contents (to be dropped into the game folder, next to nightreign.exe and nrsc_launcher.exe):
#   nrpvp_launcher.exe            double-click to play
#   NightreignPvP\nrpvp.dll       the mod
#   NightreignPvP\nrpvp.toml      settings (team size, matchmaking = true (MM-4); PvP is always on, no dev_mode)
#   NightreignPvP\me3\...         bundled me3 0.11.0 (MIT / Apache-2.0), so nothing has to be installed
#   HOW TO INSTALL.txt
# Does not rebuild nrpvp.dll: it packages dist\nrpvp.dll as it is. It does build the launcher.
# -MatchmakingTest (MM-2/MM-3 friend session): instead of the player settings, the packaged nrpvp.toml turns on developer mode with the
#   Steam probe, the matchmaking queue and the join step (dev_mode, mm_probe, mm_queue, mm_join = true, mm_join_auto = "invite+link", mm_host_create = false, mm_quiet_invite = true, mm_match_players = 2,
#   probes = false, team_size = 1).
#   The zip is dist\NightreignPvP-mmtest.zip (the player zip is not touched). Logs land in <Game>\NightreignPvP\logs as usual.
param(
    [string]$Me3Dir = 'Z:\Tools\me3',
    [int]$TeamSize = 1,
    [switch]$MatchmakingTest
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$dist = Join-Path $repo 'dist'
$dll = Join-Path $dist 'nrpvp.dll'
if (-not (Test-Path -LiteralPath $dll)) { throw 'dist\nrpvp.dll not found. Run scripts\build.ps1 first.' }
foreach ($f in 'bin\me3.exe', 'bin\me3-launcher.exe', 'bin\me3_mod_host.dll', 'LICENSE-MIT', 'LICENSE-APACHE') {
    if (-not (Test-Path -LiteralPath (Join-Path $Me3Dir $f))) { throw "me3 file missing: $Me3Dir\$f" }
}

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
$prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'   # cargo writes progress to stderr
& $cargo build --release -p nrpvp-launcher --manifest-path (Join-Path $repo 'Cargo.toml') 2>&1 | Out-Null
$code = $LASTEXITCODE; $ErrorActionPreference = $prev
if ($code -ne 0) { throw "cargo build of the launcher failed ($code)" }
$launcher = Join-Path $repo 'target\release\nrpvp_launcher.exe'

$stage = Join-Path $dist 'release-stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
$mod = Join-Path $stage 'NightreignPvP'
$me3 = Join-Path $mod 'me3'
New-Item -ItemType Directory -Force $me3 | Out-Null

Copy-Item -LiteralPath $launcher -Destination $stage
Copy-Item -LiteralPath $dll -Destination $mod
Copy-Item -LiteralPath (Join-Path $Me3Dir 'bin\me3.exe'), (Join-Path $Me3Dir 'bin\me3-launcher.exe'), (Join-Path $Me3Dir 'bin\me3_mod_host.dll') -Destination $me3
Copy-Item -LiteralPath (Join-Path $Me3Dir 'LICENSE-MIT'), (Join-Path $Me3Dir 'LICENSE-APACHE') -Destination $me3
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'release\HOW TO INSTALL.txt') -Destination $stage

$ascii = New-Object Text.ASCIIEncoding
if ($MatchmakingTest) {
    # MM-2/MM-3 test build for a friend session: developer mode (debug overlay with the Queue and join-method buttons; hotkeys F1 F2 F3 F5 as a backup).
    $toml = @(
        '# Nightreign PvP MATCHMAKING TEST settings (MM-2, MM-3). Developer build: the debug overlay (Insert hides it) has the Queue buttons.',
        'dev_mode = true',
        'mm_probe = true',
        'mm_queue = true',
        'mm_match_players = 2',
        'mm_join = true',
        'mm_join_auto = "invite+link"',
        '# MM-3e (the mod creating the lobby itself) failed in FS-MM2: the lobby must come from the game (Invite Members).',
        'mm_host_create = false',
        '# MM-3f: while the host waits at MATCH READY the Steam invite window of Invite Members is skipped (no code is patched).',
        'mm_quiet_invite = true',
        '# MM-3c lobby tracer (hardware breakpoints) is a solo developer tool: never in a friend zip.',
        'mm_lobby_trace = false',
        '# MM-3d SC-adoption test button is a solo developer tool: never in a friend zip.',
        'mm_sc_create = false',
        'probes = false',
        # Developer mode defaults to pvpve = false; without these two lines a queued match would have no PvP at all.
        'pvpve = true',
        'pvpve_scheme = "rivals"',
        'team_size = 1',
        'team_candidates = [29, 6, 3, 16, 13, 2, 1]',
        '# Old test hotkeys unbound so a tester cannot trigger them by accident (the overlay buttons remain).',
        'hotkey_downed_cycle = ""',
        'hotkey_pos_cycle = ""',
        'hotkey_pos_save = ""',
        'hotkey_pos_write = ""',
        'hotkey_write_all = ""',
        'hotkey_restore_all = ""'
    ) -join "`r`n"
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'release\MATCHMAKING TEST.txt') -Destination $stage
} else {
    $toml = @(
        '# Nightreign PvP settings. You do not need to change anything here.',
        "team_size = $TeamSize",
        '# MM-4: Solo Battle Royale matchmaking (the panel opens with the Insert key). Set to false to turn it off.',
        'matchmaking = true',
        'team_candidates = [29, 6, 3, 16, 13, 2, 1]'
    ) -join "`r`n"
}
[IO.File]::WriteAllText((Join-Path $mod 'nrpvp.toml'), $toml + "`r`n", $ascii)

$commit = (& git -C $repo rev-parse --short HEAD 2>$null)
$dirty = if (& git -C $repo status --porcelain 2>$null) { 'dirty' } else { 'clean' }
$ver = "built $((Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm')) UTC, commit $commit ($dirty), nrpvp.dll $((Get-FileHash $dll -Algorithm SHA256).Hash.Substring(0,16)), me3 0.11.0"
[IO.File]::WriteAllText((Join-Path $mod 'VERSION.txt'), $ver + "`r`n", $ascii)

$zipName = if ($MatchmakingTest) { 'NightreignPvP-mmtest.zip' } else { 'NightreignPvP.zip' }
$zip = Join-Path $dist $zipName
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
Write-Output "OK: $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
Write-Output $ver
