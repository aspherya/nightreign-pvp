# Package the friend kit: dist/friend-kit.zip. Does NOT rebuild; run ./scripts/build.ps1 first.
#   ./scripts/package-friend.ps1
[CmdletBinding()]
param(
    # Host's Seamless Co-op DLL: hashed and the hash embedded in the kit (the DLL itself is never copied).
    [string]$NrscPath = 'Z:\SteamLibrary\steamapps\common\ELDEN RING NIGHTREIGN\Game\SeamlessCoop\nrsc.dll'
)
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $repo 'dist'
$src = Join-Path $PSScriptRoot 'friend-kit'
$dll = Join-Path $dist 'nrpvp.dll'
$pdb = Join-Path $dist 'nrpvp.pdb'

if (-not (Test-Path -LiteralPath $dll)) { throw "dist\nrpvp.dll not found. Run ./scripts/build.ps1 first (this script does not build)." }
if (-not (Test-Path -LiteralPath $pdb)) { throw "dist\nrpvp.pdb not found. Run ./scripts/build.ps1 first." }
if (-not (Test-Path -LiteralPath $NrscPath)) { throw "nrsc.dll not found at $NrscPath (needed to embed the host hash). Pass -NrscPath." }
foreach ($f in 'launch-friend.ps1', 'README-FRIEND.md') {
    if (-not (Test-Path -LiteralPath (Join-Path $src $f))) { throw "Missing kit source: scripts\friend-kit\$f" }
}

$hash = (Get-FileHash -LiteralPath $NrscPath -Algorithm SHA256).Hash
$commit = (& git -C $repo rev-parse --short HEAD)
if ($LASTEXITCODE -ne 0 -or -not $commit) { throw 'git rev-parse failed.' }
$commit = "$commit".Trim()
$dirty = (& git -C $repo status --porcelain)
$dirtyFlag = if ($dirty) { 'dirty' } else { 'clean' }
$stamp = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss') + ' UTC'

$stageRoot = Join-Path $dist 'friend-kit-stage'
$stage = Join-Path $stageRoot 'nrpvp-friend-kit'
if (Test-Path -LiteralPath $stageRoot) { Remove-Item -LiteralPath $stageRoot -Recurse -Force }
New-Item -ItemType Directory -Path $stage | Out-Null

Copy-Item -LiteralPath $dll, $pdb -Destination $stage
# nrpvp.toml is intentionally not shipped: the DLL writes the current default config next to itself on first run
# (a copy from dist\ may be stale or hold local edits).
# Placeholders are replaced with the host's values. UTF-8 with BOM so Windows PowerShell 5.1 reads the .ps1 right.
$utf8Bom = New-Object System.Text.UTF8Encoding($true)
foreach ($f in 'launch-friend.ps1', 'README-FRIEND.md') {
    $text = [IO.File]::ReadAllText((Join-Path $src $f))
    $text = $text.Replace('@@NRSC_SHA256@@', $hash)
    [IO.File]::WriteAllText((Join-Path $stage $f), $text, $utf8Bom)
}
$dllHash = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash
$version = @"
nrpvp friend kit
build timestamp: $stamp
git commit: $commit ($dirtyFlag working tree)
nrpvp.dll SHA256: $dllHash
host nrsc.dll SHA256: $hash
"@
[IO.File]::WriteAllText((Join-Path $stage 'VERSION.txt'), $version, $utf8Bom)

$zip = Join-Path $dist 'friend-kit.zip'
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
Remove-Item -LiteralPath $stageRoot -Recurse -Force

Write-Host "Built $zip ($commit, $dirtyFlag)"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$z = [IO.Compression.ZipFile]::OpenRead($zip)
try { $z.Entries | ForEach-Object { Write-Host ("  {0}  ({1} bytes)" -f $_.FullName, $_.Length) } } finally { $z.Dispose() }
