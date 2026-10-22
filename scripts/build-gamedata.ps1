# Build the game-data package dist\gamedata\ (me3 [[packages]] folder, game-relative paths) from the unpacked game and our sources.
# Usage: ./scripts/build-gamedata.ps1 [-Unpacked Z:\Tools\nr-unpacked\1.03.3] [-Witchy Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe]
#
# What it builds:
#   msg\<lang>\menu_dlc01.msgbnd.dcx for every language (UI-1): the game's bundle with our entries from gamedata\fmg-entries.json added.
#   Only the FMGs named in that file are rewritten (WitchyBND turns whitespace-only texts into empty ones, so FMGs that hold such
#   texts are refused); every other file of the bundle is passed through.
#   event\m10_00_00_00.emevd.dcx (UI-2): the Roundtable's event script with our events from gamedata\emevd-events.json appended by
#   scripts\emevd_add_events.py (needs Python 3; the default install path is tried, then `python` on PATH).
#   map\mapstudio\m60_00_00_99.msb.dcx (HAWK-2, the hawk crossing): every start-hawk route replaced by the one crossing line
#   (.claude\experiments\HAWK-0\patch_routes.py, needs Python 3 and the game's oo2core_9_win64.dll). In the player zip too.
# Game content stays out of git: dist\ is gitignored. The unpack must be game 1.03.3 (pinned hash below); after a game patch, re-unpack,
# check the result in game, then update the pin.
[CmdletBinding()]
param(
    [string]$Unpacked = 'Z:\Tools\nr-unpacked\1.03.3',
    [string]$Witchy = 'Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe',
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
$pinnedEmevd = @{ 'event/m10_00_00_00.emevd.dcx' = 'A4D87CAEBE834496F66B958E1FCDE0190006CB97ACE6805AB1A02A00C642D55B' }
$eventsFile = Join-Path $repo 'gamedata\emevd-events.json'

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

# UI-2: our event-script events.
$ev = Get-Content $eventsFile -Raw | ConvertFrom-Json
if (@($ev.events).Count -gt 0) {
    $py = Join-Path $env:LOCALAPPDATA 'Programs\Python\Python312\python.exe'
    if (-not (Test-Path $py)) { $py = (Get-Command python -ErrorAction SilentlyContinue).Source }
    if (-not $py) { throw 'Python 3 not found (needed for scripts\emevd_add_events.py)' }
    $rel = $ev.file
    $src = Join-Path $Unpacked ($rel -replace '/', '\')
    if (-not (Test-Path $src)) { throw "Unpacked game file not found: $src" }
    $hh = (Get-FileHash $src).Hash
    if ($pinnedEmevd[$rel] -ne $hh) { throw "$rel hash $hh is not the pinned 1.03.3 hash" }
    $ew = Join-Path $work 'event'
    New-Item -ItemType Directory -Force $ew | Out-Null
    $dcx = Join-Path $ew (Split-Path $src -Leaf)
    Copy-Item $src $dcx
    Invoke-Witchy @($dcx)
    $plain = $dcx -replace '\.dcx$', ''
    & $py -I (Join-Path $PSScriptRoot 'emevd_add_events.py') $plain $eventsFile
    if ($LASTEXITCODE -ne 0) { throw "emevd_add_events.py failed ($LASTEXITCODE)" }
    Remove-Item $dcx
    Invoke-Witchy @($plain)
    $dst = Join-Path $out (Split-Path ($rel -replace '/', '\') -Parent)
    New-Item -ItemType Directory -Force $dst | Out-Null
    Copy-Item $dcx $dst
    Write-Host ("{0}  {1}  {2}  ({3} events)" -f $rel, (Get-Item $dcx).Length, (Get-FileHash $dcx).Hash.Substring(0, 16), @($ev.events).Count)
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
    & $py -I (Join-Path $repo '.claude\experiments\HAWK-0\patch_routes.py') @routeArgs
    if ($LASTEXITCODE -ne 0) { throw "patch_routes.py failed ($LASTEXITCODE)" }
    Write-Host "HAWK-2: start routes ($(if ($HawkLine) { "one line $HawkLine" } else { 'stretched' })) in $dst"
}
