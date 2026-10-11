# Builds dist\NightreignPvP.zip: the drag-and-drop install for players who already have Seamless Co-op.
# Contents (to be dropped into the game folder, next to nightreign.exe and nrsc_launcher.exe):
#   nrpvp_launcher.exe            double-click to play
#   NightreignPvP\nrpvp.dll       the mod
#   NightreignPvP\nrpvp.toml      settings (team size and defaults; PvP is always on, no dev_mode)
#   NightreignPvP\me3\...         bundled me3 0.11.0 (MIT / Apache-2.0), so nothing has to be installed
#   HOW TO INSTALL.txt
# Does not rebuild nrpvp.dll: it packages dist\nrpvp.dll as it is. It does build the launcher.
param(
    [string]$Me3Dir = 'Z:\Tools\me3',
    [int]$TeamSize = 1
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
$toml = @(
    '# Nightreign PvP settings. You do not need to change anything here.',
    "team_size = $TeamSize",
    'team_candidates = [29, 6, 3, 16, 13, 2, 1]'
) -join "`r`n"
[IO.File]::WriteAllText((Join-Path $mod 'nrpvp.toml'), $toml + "`r`n", $ascii)

$commit = (& git -C $repo rev-parse --short HEAD 2>$null)
$dirty = if (& git -C $repo status --porcelain 2>$null) { 'dirty' } else { 'clean' }
$ver = "built $((Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm')) UTC, commit $commit ($dirty), nrpvp.dll $((Get-FileHash $dll -Algorithm SHA256).Hash.Substring(0,16)), me3 0.11.0"
[IO.File]::WriteAllText((Join-Path $mod 'VERSION.txt'), $ver + "`r`n", $ascii)

$zip = Join-Path $dist 'NightreignPvP.zip'
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item $stage -Recurse -Force
Write-Output "OK: $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB)"
Write-Output $ver
