# nightreign-pvp friend kit launcher (Windows PowerShell 5.1 compatible).
# Finds the game, checks Seamless Co-op, writes a me3 profile next to this script and launches the game
# through me3 (no EAC, official servers blocked). NEVER go online with a modded game.
#
#   powershell -ExecutionPolicy Bypass -File launch-friend.ps1
#   ... -GameDir 'D:\Games\ELDEN RING NIGHTREIGN\Game'   # override auto-detection
#   ... -DryRun                                          # print everything, write nothing, launch nothing
#   ... -SelfTest                                        # test the libraryfolders.vdf parser
[CmdletBinding()]
param(
    [string]$GameDir,
    [switch]$DryRun,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'

$NightreignAppId = '2622380'
# Filled in by package-friend.ps1 with the host's nrsc.dll SHA256. If still the placeholder, the hash is unknown.
$ExpectedNrscSha256 = '@@NRSC_SHA256@@'
$ExpectedNrscVersion = 'Seamless Co-op for Nightreign v1.1.3 (Nexus 1.13)'

# Pure function: returns the library paths (unescaped) from libraryfolders.vdf text.
#   WithApp = libraries whose "apps" block lists $AppId; All = every library path.
function Get-SteamLibrariesWithApp {
    param([string]$VdfText, [string]$AppId)
    $found = @()
    $all = @()
    $currentPath = $null
    $inApps = $false
    $appsDepth = 0
    $depth = 0
    foreach ($line in ($VdfText -split "`r?`n")) {
        $t = $line.Trim()
        if ($t -eq '{') {
            $depth++
            if ($inApps -and $appsDepth -eq 0) { $appsDepth = $depth }
            continue
        }
        if ($t -eq '}') {
            if ($inApps -and $depth -eq $appsDepth) { $inApps = $false; $appsDepth = 0 }
            $depth--
            continue
        }
        if ($t -match '^"path"\s+"(.*)"\s*$') {
            $currentPath = $Matches[1].Replace('\\', '\')
            $all += $currentPath
            continue
        }
        if ($t -match '^"apps"\s*$') { $inApps = $true; $appsDepth = 0; continue }
        if ($inApps -and $appsDepth -gt 0 -and $t -match '^"(\d+)"\s+"') {
            if ($Matches[1] -eq $AppId -and $currentPath) { $found += $currentPath }
        }
    }
    return [pscustomobject]@{ WithApp = @($found); All = @($all) }
}

if ($SelfTest) {
    $vdf = @'
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"123"
			"1245620"		"456"
		}
	}
	"1"
	{
		"path"		"Z:\\SteamLibrary"
		"label"		""
		"apps"
		{
			"2622380"		"99999"
			"1086940"		"1"
		}
	}
}
'@
    $fails = 0
    function Check($name, $cond) {
        if ($cond) { Write-Host "PASS  $name" } else { Write-Host "FAIL  $name"; $script:fails++ }
    }
    $r = Get-SteamLibrariesWithApp -VdfText $vdf -AppId $NightreignAppId
    Check 'finds app 2622380 in the second library' ($r.WithApp.Count -eq 1 -and $r.WithApp[0] -eq 'Z:\SteamLibrary')
    Check 'lists both library paths' ($r.All.Count -eq 2 -and $r.All[0] -eq 'C:\Program Files (x86)\Steam')
    $r2 = Get-SteamLibrariesWithApp -VdfText $vdf -AppId '228980'
    Check 'app in first library is attributed to the first library' ($r2.WithApp.Count -eq 1 -and $r2.WithApp[0] -eq 'C:\Program Files (x86)\Steam')
    $r3 = Get-SteamLibrariesWithApp -VdfText $vdf -AppId '1'
    Check 'a size value is not mistaken for an app id' ($r3.WithApp.Count -eq 0)
    $r4 = Get-SteamLibrariesWithApp -VdfText '' -AppId $NightreignAppId
    Check 'empty text yields nothing' ($r4.WithApp.Count -eq 0 -and $r4.All.Count -eq 0)
    if ($fails -gt 0) { Write-Host "$fails self-test(s) FAILED"; exit 1 }
    Write-Host 'All self-tests passed.'
    exit 0
}

$gameRel = 'steamapps\common\ELDEN RING NIGHTREIGN\Game'

function Find-GameDir {
    $steamPath = $null
    try { $steamPath = (Get-ItemProperty -Path 'HKCU:\Software\Valve\Steam' -ErrorAction Stop).SteamPath } catch { }
    if (-not $steamPath) { return $null }
    $steamPath = $steamPath.Replace('/', '\')
    $libs = @($steamPath)
    $vdfPath = Join-Path $steamPath 'steamapps\libraryfolders.vdf'
    if (Test-Path -LiteralPath $vdfPath) {
        $parsed = Get-SteamLibrariesWithApp -VdfText (Get-Content -LiteralPath $vdfPath -Raw) -AppId $NightreignAppId
        $libs = @($parsed.WithApp) + @($parsed.All) + $libs
    }
    foreach ($lib in $libs) {
        $cand = Join-Path $lib $gameRel
        if (Test-Path -LiteralPath (Join-Path $cand 'nightreign.exe')) { return $cand }
    }
    return $null
}

function Test-GameDir($dir) {
    return ($dir -and (Test-Path -LiteralPath (Join-Path $dir 'nightreign.exe')))
}

Write-Host '== nightreign-pvp friend kit =='
Write-Host 'Reminder: NEVER go online with this build, and do not mix it with vanilla play.'

if (-not $GameDir) { $GameDir = Find-GameDir }
if (-not (Test-GameDir $GameDir)) {
    if ($GameDir) { Write-Host "nightreign.exe not found in: $GameDir" }
    else { Write-Host 'Could not find the Nightreign game folder automatically.' }
    $GameDir = (Read-Host 'Paste the full path of the folder that contains nightreign.exe (the "Game" folder)').Trim('"', ' ')
}
if (-not (Test-GameDir $GameDir)) { throw "nightreign.exe not found in '$GameDir'. Cannot continue." }
Write-Host "Game folder: $GameDir"

$nrsc = Join-Path $GameDir 'SeamlessCoop\nrsc.dll'
if (-not (Test-Path -LiteralPath $nrsc)) {
    throw "SeamlessCoop\nrsc.dll not found under the game folder. Install $ExpectedNrscVersion first (see README-FRIEND.md)."
}
$hash = (Get-FileHash -LiteralPath $nrsc -Algorithm SHA256).Hash
Write-Host "nrsc.dll SHA256: $hash"
if ($ExpectedNrscSha256 -like '@@*') {
    Write-Host 'Host hash unknown (kit not built by package-friend.ps1). Compare the hash above with the host by hand.'
} elseif ($hash -eq $ExpectedNrscSha256) {
    Write-Host 'nrsc.dll matches the host build.'
} else {
    Write-Host 'WARNING: nrsc.dll does NOT match the host build!'
    Write-Host "  host:  $ExpectedNrscSha256"
    Write-Host "  yours: $hash"
    Write-Host "  Install exactly $ExpectedNrscVersion. Continuing anyway."
}

$kit = $PSScriptRoot
$dll = Join-Path $kit 'nrpvp.dll'
if (-not (Test-Path -LiteralPath $dll)) { throw "nrpvp.dll not found next to this script ($kit). Re-extract the whole zip." }

$me3 = $null
$cmd = Get-Command me3 -ErrorAction SilentlyContinue
if ($cmd) { $me3 = $cmd.Source }
if (-not $me3) {
    $cands = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\me3\bin\me3.exe'),
        (Join-Path $env:LOCALAPPDATA 'garyttierney\me3\bin\me3.exe'),
        (Join-Path $env:ProgramFiles 'me3\bin\me3.exe'),
        (Join-Path $env:ProgramFiles 'garyttierney\me3\bin\me3.exe')
    )
    foreach ($c in $cands) { if ($c -and (Test-Path -LiteralPath $c)) { $me3 = $c; break } }
}
if (-not $me3) {
    if ($DryRun) { Write-Host 'me3.exe NOT FOUND (dry run continues).'; $me3 = 'me3' }
    else { throw 'me3 not found on PATH or in common install locations. Install me3 0.11.0 (see README-FRIEND.md), then open a new PowerShell window.' }
}
Write-Host "me3: $me3"

foreach ($p in @($nrsc, $dll)) {
    if ($p.Contains("'")) { throw "Path contains a single quote, which this launcher cannot put in a TOML literal string: $p" }
}
$profileText = @"
# Generated by launch-friend.ps1. Do not edit; it is rewritten on every launch.
profileVersion = "v1"

[[supports]]
game = "nightreign"

[[natives]]
path = '$nrsc'
load_early = true

[[natives]]
path = '$dll'
load_after = [{ id = "nrsc.dll", optional = false }]
"@
$profilePath = Join-Path $kit 'nrpvp-friend.me3.toml'
$me3Args = @('launch', '-g', 'nightreign', '-p', $profilePath)

Write-Host "Profile: $profilePath"
Write-Host '----- profile contents -----'
Write-Host $profileText
Write-Host '----------------------------'
Write-Host "Command: $me3 $($me3Args -join ' ')"

if ($DryRun) { Write-Host 'Dry run: profile not written, game not launched.'; exit 0 }

Set-Content -LiteralPath $profilePath -Value $profileText -Encoding UTF8
Write-Host 'Launching... (logs are written to the logs folder next to this script)'
& $me3 @me3Args
exit $LASTEXITCODE
