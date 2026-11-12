# Build the game-data package dist\gamedata\ (me3 [[packages]] folder, game-relative paths) from the unpacked game and our sources.
# Usage: ./scripts/build-gamedata.ps1 [-Unpacked Z:\Tools\nr-unpacked\1.03.3] [-Witchy Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe]
#
# What it builds:
#   msg\<lang>\menu_dlc01.msgbnd.dcx for every language (UI-1): the game's bundle with our entries from gamedata\fmg-entries.json added.
#   Only the FMGs named in that file are rewritten (WitchyBND turns whitespace-only texts into empty ones, so FMGs that hold such
#   texts are refused); every other file of the bundle is passed through.
#   event\m10_00_00_00.emevd.dcx (UI-2): the Roundtable's event script with our events from gamedata\emevd-events.json appended by
#   scripts\emevd_add_events.py (needs Python 3; the default install path is tried, then `python` on PATH).
#   event\common.emevd.dcx (OFF-4): the event script every map runs, with our bot events from gamedata\emevd-bots.json (the game's own
#   NPC bot instructions 2003:97 / 2003:98, started by our flags 10007300+; research\OFF-3-bots.md).
#   event\common.emevd.dcx + event\common_func.emevd.dcx (M5-3, research\M5-no-boss-nights.md): nights with no boss and no minions
#   (gamedata\emevd-nights-func.json replaces the shared boss/minion events, gamedata\emevd-nights.json adds the timed night end).
#   Several event files may target one game file: they are applied in the order of $eventsFiles. Files marked dev-only (the Arena events,
#   ARENA-0..3) are left out of dist\gamedata-player\, the complete PLAYER package that package-release.ps1 zips: every file of
#   dist\gamedata, event scripts rebuilt without their dev-only events, and game files that only dev-only events touch (m18) left out.
#   map\mapstudio\m60_00_00_99.msb.dcx (HAWK-2, the hawk crossing): every start-hawk route replaced by the one crossing line
#   (.claude\archive\experiments\HAWK-0\patch_routes.py, needs Python 3 and the game's oo2core_9_win64.dll). In the player zip too.
#   regulation.bin (UI-3, the Battle Royale card in the expedition list): the pinned regulation.bin with the param rows of
#   gamedata\nightbossmenu-rows.json (NightBossMenuParam row 9 replaced) and gamedata\playarea-rows.json (TIDE-MAX: the final circle's
#   radius, PlayAreaCreateCommonParam row 0), written by tools\regulation (sbwriter: Smithbox's own
#   SoulsFormats writer; needs the .NET 10 SDK and a Smithbox 2.2.6 folder, -Smithbox). Only Smithbox's writer is verified to make a
#   file the game loads: WitchyBND's regulation repack and our earlier Python writer both make the game close (TRAPS.md).
#   Sits at the package root, where me3 picks it up; in the player zip too.
# Game content stays out of git: dist\ is gitignored. The unpack must be game 1.03.3 (pinned hash below); after a game patch, re-unpack,
# check the result in game, then update the pin.
[CmdletBinding()]
param(
    [string]$Unpacked = 'Z:\Tools\nr-unpacked\1.03.3',
    [string]$Witchy = 'Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe',
    # UI-3: Smithbox install whose SoulsFormats DLLs build regulation.bin (referenced at build time, never copied into the repo).
    [string]$Smithbox = 'Z:\Tools\Smithbox-2.2.6',
    # HAWK-2: the crossing line is built by default (it is part of the game). -VanillaRoutes leaves map\ out (vanilla 200 m start
    # routes; dev comparisons only: package-release.ps1 refuses such a package).
    [switch]$VanillaRoutes,
    # HAWK-1b's opt-in switch; kept so old command lines still work. No effect: the routes are always built now.
    [switch]$HawkRoutes,
    # The line: start; waypoint; end as x,y,z in the m60_00_00_99 frame. Default = diagonal "B" (Leonardo 2026-10-22: the longer and
    # higher one), heights over every map variant (diagonal.py). Empty = the old per-route stretch of HAWK-1b runs 1-2.
    [string]$HawkLine = '858,210,678;512,250,1024;166,448,1370'
)
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$out = Join-Path $repo 'dist\gamedata'
$work = Join-Path $env:TEMP 'nrpvp-gamedata'
$entriesFile = Join-Path $repo 'gamedata\fmg-entries.json'
# engus menu_dlc01.msgbnd.dcx of game 1.03.3 (UI-0 step 2).
$pinned = 'E33F1C338878DA423D95CF54589072D99425F2A92C10603E5C93BDA9323184DB'
# event\m10_00_00_00.emevd.dcx of game 1.03.3 (UI-0 step 2).
# event\common.emevd.dcx of game 1.03.3 (OFF-4). event\common_func.emevd.dcx of game 1.03.3 (M5-3).
$pinnedEmevd = @{
    'event/m10_00_00_00.emevd.dcx' = 'A4D87CAEBE834496F66B958E1FCDE0190006CB97ACE6805AB1A02A00C642D55B'
    'event/common.emevd.dcx'       = '0018635B07959514CD9F1DF4E4889959B649C04E175567E6D8A8E2F11B0E2805'
    'event/common_func.emevd.dcx'  = '374186FCC2782FA9EABCEEE4F8ADA8854C50DB309CC7D476D262EF3B7B105794'
    # event\m18_00_00_00.emevd.dcx of game 1.03.3 (ARENA-0: Heolstor's arena).
    'event/m18_00_00_00.emevd.dcx' = '685531433FB79CB955E9D6B864098128E9DBD0097610E5AA6B3C1FDCC1A4FED4'
}
# Our event scripts (each names its target in "file"), applied in this order; Dev = left out of the player copy.
$eventsFiles = @(
    @{ Path = (Join-Path $repo 'gamedata\emevd-events.json'); Dev = $false },
    # BOT-5 (2026-11-06): the bot events are a player feature (Bots On in Matchmaking Settings), so no longer dev-only.
    @{ Path = (Join-Path $repo 'gamedata\emevd-bots.json'); Dev = $false },
    @{ Path = (Join-Path $repo 'gamedata\emevd-nights.json'); Dev = $false },
    @{ Path = (Join-Path $repo 'gamedata\emevd-nights-func.json'); Dev = $false },
    # ARENA-0 (2026-11-08): the warp to Heolstor's arena (our flag 10007500), dev only until the Arena mode is built.
    @{ Path = (Join-Path $repo 'gamedata\emevd-arena.json'); Dev = $true },
    # ARENA-0 1b: Heolstor kept off in his arena (m18), dev only.
    @{ Path = (Join-Path $repo 'gamedata\emevd-arena-m18.json'); Dev = $true },
    # ARENA-0: Heolstor's fight never starts (no 7511, no boss bar), dev only.
    @{ Path = (Join-Path $repo 'gamedata\emevd-arena-func.json'); Dev = $true }
)
$outPlayer = Join-Path $repo 'dist\gamedata-player'

if (-not (Test-Path $Witchy)) { throw "WitchyBND not found at $Witchy" }
$engus = Join-Path $Unpacked 'msg\engus\menu_dlc01.msgbnd.dcx'
if (-not (Test-Path $engus)) { throw "Unpacked game not found: $engus" }
$h = (Get-FileHash $engus).Hash
if ($h -ne $pinned) { throw "msg\engus\menu_dlc01.msgbnd.dcx hash $h is not the pinned 1.03.3 hash; re-check after a game patch before building" }

$entries = Get-Content $entriesFile -Raw | ConvertFrom-Json
$fmgs = @($entries.PSObject.Properties | Where-Object { $_.Name -notlike '_*' })

function Invoke-Witchy([string[]]$paths) {
    $args = @('-s', '-p') + ($paths | ForEach-Object { "`"$_`"" })
    $p = Start-Process $Witchy -ArgumentList $args -Wait -PassThru -NoNewWindow
    if ($p.ExitCode -ne 0) { throw "WitchyBND failed ($($p.ExitCode)) on $($paths -join ', ')" }
}

function Escape-Xml([string]$s) { [System.Security.SecurityElement]::Escape($s) }

if (Test-Path $work) { Remove-Item -Recurse -Force $work }
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
if (Test-Path $outPlayer) { Remove-Item -Recurse -Force $outPlayer }
New-Item -ItemType Directory -Force $work, $out | Out-Null

$langs = Get-ChildItem (Join-Path $Unpacked 'msg') -Directory | Where-Object { Test-Path (Join-Path $_.FullName 'menu_dlc01.msgbnd.dcx') }
foreach ($lang in $langs) {
    $lw = Join-Path $work $lang.Name
    New-Item -ItemType Directory -Force $lw | Out-Null
    $bnd = Join-Path $lw 'menu_dlc01.msgbnd.dcx'
    Copy-Item (Join-Path $lang.FullName 'menu_dlc01.msgbnd.dcx') $bnd
    Invoke-Witchy @($bnd)
    $dir = Join-Path $lw 'menu_dlc01-msgbnd-dcx'
    foreach ($f in $fmgs) {
        $fmg = Join-Path $dir "$($f.Name).fmg"
        if (-not (Test-Path $fmg)) { throw "$($lang.Name): $($f.Name).fmg not in the bundle" }
        Invoke-Witchy @($fmg)
        $xmlPath = "$fmg.xml"
        $xml = [System.IO.File]::ReadAllText($xmlPath)
        if ($xml -match '<text id="\d+">\s+</text>') { throw "$($lang.Name): $($f.Name) has whitespace-only texts; WitchyBND would empty them. Use another FMG." }
        $add = ''
        foreach ($e in $f.Value.PSObject.Properties) {
            if ($xml -match "<text id=`"$($e.Name)`">") { throw "$($lang.Name): $($f.Name) already has id $($e.Name)" }
            $add += "<text id=`"$($e.Name)`">$(Escape-Xml $e.Value)</text>`n"
        }
        $i = $xml.LastIndexOf('</entries>')
        if ($i -lt 0) { throw "$($lang.Name): no </entries> in $xmlPath" }
        $xml = $xml.Substring(0, $i) + $add + $xml.Substring($i)
        [System.IO.File]::WriteAllText($xmlPath, $xml, (New-Object System.Text.UTF8Encoding $false))
        Remove-Item $fmg
        Invoke-Witchy @($xmlPath)
        Remove-Item $xmlPath
    }
    Remove-Item $bnd
    Invoke-Witchy @($dir)
    $dst = Join-Path $out "msg\$($lang.Name)"
    New-Item -ItemType Directory -Force $dst | Out-Null
    Copy-Item $bnd $dst
    Write-Host ("{0,-6} {1}  {2}" -f $lang.Name, (Get-Item $bnd).Length, (Get-FileHash $bnd).Hash.Substring(0, 16))
}
Write-Host "OK: $out ($($langs.Count) languages; FMGs: $(($fmgs | ForEach-Object Name) -join ', '))"

# UI-2 / OFF-4 / M5-3: our event-script events, grouped by the game file they change.
$py = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\python.exe'
if (-not (Test-Path $py)) { $py = (Get-Command python -ErrorAction SilentlyContinue).Source }
if (-not $py) { throw 'Python 3 not found (needed for scripts\emevd_add_events.py)' }
$byFile = [ordered]@{}
foreach ($e in $eventsFiles) {
    $rel = (Get-Content $e.Path -Raw | ConvertFrom-Json).file
    if (-not $byFile.Contains($rel)) { $byFile[$rel] = @() }
    $byFile[$rel] += $e
}
# Build one game file from the pinned original with the given event files; returns the .dcx path.
function Build-Emevd([string]$rel, $files, [string]$tag) {
    $src = Join-Path $Unpacked ($rel -replace '/', '\')
    if (-not (Test-Path $src)) { throw "Unpacked game file not found: $src" }
    $hh = (Get-FileHash $src).Hash
    if ($pinnedEmevd[$rel] -ne $hh) { throw "$rel hash $hh is not the pinned 1.03.3 hash" }
    $ew = Join-Path $work "event-$tag"
    New-Item -ItemType Directory -Force $ew | Out-Null
    $dcx = Join-Path $ew (Split-Path $src -Leaf)
    Copy-Item $src $dcx
    Invoke-Witchy @($dcx)
    $plain = $dcx -replace '\.dcx$', ''
    foreach ($f in $files) {
        & $py -I (Join-Path $PSScriptRoot 'emevd_add_events.py') $plain $f.Path | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "emevd_add_events.py failed ($LASTEXITCODE) on $($f.Path)" }
    }
    Remove-Item $dcx
    Invoke-Witchy @($plain)
    return $dcx
}
$devOnlyRels = @()
foreach ($rel in $byFile.Keys) {
    $files = $byFile[$rel]
    $dcx = Build-Emevd $rel $files 'dev'
    $dst = Join-Path $out (Split-Path ($rel -replace '/', '\') -Parent)
    New-Item -ItemType Directory -Force $dst | Out-Null
    Copy-Item $dcx $dst
    Write-Host ("{0}  {1}  {2}  ({3} event file(s))" -f $rel, (Get-Item $dcx).Length, (Get-FileHash $dcx).Hash.Substring(0, 16), $files.Count)
    $player = @($files | Where-Object { -not $_.Dev })
    if ($player.Count -ne $files.Count -and $player.Count -gt 0) {
        $pdcx = Build-Emevd $rel $player 'player'
        $pdst = Join-Path $outPlayer (Split-Path ($rel -replace '/', '\') -Parent)
        New-Item -ItemType Directory -Force $pdst | Out-Null
        Copy-Item $pdcx $pdst
        Write-Host ("{0}  {1}  {2}  (player copy, {3} event file(s), no dev-only events)" -f $rel, (Get-Item $pdcx).Length, (Get-FileHash $pdcx).Hash.Substring(0, 16), $player.Count)
    }
    if ($player.Count -eq 0) { $devOnlyRels += ($rel -replace '/', '\') }
}

# HAWK-1b/HAWK-2: the crossing line for every start-hawk route (TECHNICAL_NOTES 4c, PROJECT_PLAN 13 B1).
if ($HawkRoutes) { Write-Host '-HawkRoutes is the default now (HAWK-2); the switch has no effect' }
if (-not $VanillaRoutes) {
    $py = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\python.exe'
    if (-not (Test-Path $py)) { $py = (Get-Command python -ErrorAction SilentlyContinue).Source }
    if (-not $py) { throw 'Python 3 not found (needed for patch_routes.py)' }
    $msb = Join-Path $Unpacked 'map\mapstudio\m60_00_00_99.msb.dcx'
    # map\mapstudio\m60_00_00_99.msb.dcx of game 1.03.3 (HAWK-0).
    $hh = (Get-FileHash $msb).Hash
    if ($hh -ne '7338A794A4FF79C02B65D378607B1F0874BDF2ED1FC4885002432414B5B9EA9C') { throw "m60_00_00_99.msb.dcx hash $hh is not the pinned 1.03.3 hash" }
    $oodle = 'Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\oo2core_9_win64.dll'
    $dst = Join-Path $out 'map\mapstudio\m60_00_00_99.msb.dcx'
    $routeArgs = @($oodle, $msb, $dst)
    if ($HawkLine) { $routeArgs += @('--single', $HawkLine) }
    & $py -I (Join-Path $repo '.claude\archive\experiments\HAWK-0\patch_routes.py') @routeArgs
    if ($LASTEXITCODE -ne 0) { throw "patch_routes.py failed ($LASTEXITCODE)" }
    Write-Host "HAWK-2: start routes ($(if ($HawkLine) { "one line $HawkLine" } else { 'stretched' })) in $dst"
}

# UI-3: regulation.bin with our param rows, written by Smithbox's own writer (tools\regulation, sbwriter; it checks the pinned hash too).
$reg = Join-Path $Unpacked 'regulation.bin'
# regulation.bin of game 1.03.3 (UI-3).
$hh = (Get-FileHash $reg).Hash
if ($hh -ne '876A3CA279A4561D0C69F81FE5E510C75C59AB9A8A201694F8CF4A42C91E0268') { throw "regulation.bin hash $hh is not the pinned 1.03.3 hash" }
$dotnet = (Get-Command dotnet -ErrorAction SilentlyContinue).Source
if (-not $dotnet) { throw 'dotnet not found: regulation.bin needs the .NET 10 SDK (https://dotnet.microsoft.com/download/dotnet/10.0)' }
if (-not (& $dotnet --list-sdks | Where-Object { $_ -match '^10\.' })) { throw 'No .NET 10 SDK installed (dotnet --list-sdks): sbwriter targets net10.0 like Smithbox 2.2.6' }
foreach ($dll in 'Andre.SoulsFormats.dll', 'Andre.Core.dll', 'Andre.IO.dll', 'DotNext.dll', 'DotNext.IO.dll', 'DotNext.Unsafe.dll', 'CommunityToolkit.HighPerformance.dll', 'ZstdNet.dll', 'libzstd.dll', 'oo2core_9_win64.dll') {
    if (-not (Test-Path (Join-Path $Smithbox $dll))) { throw "Smithbox DLL missing: $Smithbox\$dll (install Smithbox 2.2.6 or pass -Smithbox <folder>)" }
}
# Build in the work folder (no bin\obj in the repo).
$sbSrc = Join-Path $work 'sbwriter'
New-Item -ItemType Directory -Force $sbSrc | Out-Null
Copy-Item (Join-Path $repo 'tools\regulation\sbwriter.csproj'), (Join-Path $repo 'tools\regulation\Program.cs') $sbSrc
$sbBin = Join-Path $sbSrc 'bin'
$prev = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
$buildLog = & $dotnet build (Join-Path $sbSrc 'sbwriter.csproj') -c Release -o $sbBin "-p:SmithboxDir=$Smithbox" -nologo 2>&1
$code = $LASTEXITCODE; $ErrorActionPreference = $prev
if ($code -ne 0) { $buildLog | Out-Host; throw "dotnet build of tools\regulation\sbwriter failed ($code)" }
$regOut = Join-Path $out 'regulation.bin'
& (Join-Path $sbBin 'sbwriter.exe') $reg (Join-Path $repo 'gamedata\nightbossmenu-rows.json') (Join-Path $repo 'gamedata\playarea-rows.json') $regOut
if ($LASTEXITCODE -ne 0) { throw "sbwriter failed ($LASTEXITCODE)" }
Write-Host ("UI-3: regulation.bin  {0}  {1}" -f (Get-Item $regOut).Length, (Get-FileHash $regOut).Hash.Substring(0, 16))

# Title menu background, generated from our bitmap and the pinned original movie (no game GFx is stored in git).
& $py -I (Join-Path $repo 'tools\gfx\title_background.py') (Join-Path $Unpacked 'menu\05_000_title.gfx') (Join-Path $out 'menu\05_000_title.gfx')
if ($LASTEXITCODE -ne 0) { throw 'Title background build failed; check the pinned title movie and image assets.' }

# The player package (dist\gamedata-player, zipped by package-release.ps1): every other file of dist\gamedata as it is, except game
# files that only dev-only events change (they stay vanilla for players).
$outFull = (Resolve-Path $out).Path
foreach ($f in Get-ChildItem -LiteralPath $out -Recurse -File) {
    $rel = $f.FullName.Substring($outFull.Length + 1)
    if ($devOnlyRels -contains $rel) { continue }
    $dst = Join-Path $outPlayer $rel
    if (Test-Path -LiteralPath $dst) { continue }
    New-Item -ItemType Directory -Force (Split-Path $dst -Parent) | Out-Null
    Copy-Item -LiteralPath $f.FullName $dst
}
$nPlayer = @(Get-ChildItem -LiteralPath $outPlayer -Recurse -File).Count
Write-Host ("player package: {0}  {1} file(s), dev-only left out: {2}" -f $outPlayer, $nPlayer, $(if ($devOnlyRels) { $devOnlyRels -join ', ' } else { 'none' }))
