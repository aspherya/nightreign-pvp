# Build the game-data package dist\gamedata\ (me3 [[packages]] folder, game-relative paths) from the unpacked game and our sources.
# Usage: ./scripts/build-gamedata.ps1 [-Unpacked Z:\Tools\nr-unpacked\1.03.3] [-Witchy Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe]
#
# What it builds (UI-1):
#   msg\<lang>\menu_dlc01.msgbnd.dcx for every language: the game's bundle with our entries from gamedata\fmg-entries.json added.
#   Only the FMGs named in that file are rewritten (WitchyBND turns whitespace-only texts into empty ones, so FMGs that hold such
#   texts are refused); every other file of the bundle is passed through.
# Game content stays out of git: dist\ is gitignored. The unpack must be game 1.03.3 (pinned hash below); after a game patch, re-unpack,
# check the result in game, then update the pin.
[CmdletBinding()]
param(
    [string]$Unpacked = 'Z:\Tools\nr-unpacked\1.03.3',
    [string]$Witchy = 'Z:\Tools\WitchyBND-3.0.1.0\WitchyBND.exe'
)
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$out = Join-Path $repo 'dist\gamedata'
$work = Join-Path $env:TEMP 'nrpvp-gamedata'
$entriesFile = Join-Path $repo 'gamedata\fmg-entries.json'
# engus menu_dlc01.msgbnd.dcx of game 1.03.3 (UI-0 step 2).
$pinned = 'E33F1C338878DA423D95CF54589072D99425F2A92C10603E5C93BDA9323184DB'

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
