# Packs the pinned inputs CI needs to build the game data itself (2026-11-13): the original game 1.03.3 files our game data is made
# from, the tools that rebuild them (WitchyBND, ESDLang 0.5.1, the Smithbox 2.2.6 DLLs regulation.bin needs) and me3.
# Nothing is compiled here: CI runs build-gamedata.ps1 on every release, so a change to gamedata\ or tools\loadouts\ ships by itself.
# Upload dist/ci-inputs.zip to a DRAFT release in this repository (it holds game files: never publish it), then update
# pins/ci-inputs.json with that draft's release ID and the printed SHA-256. Redo this only after a game patch or a tool update.
param(
    [Parameter(Mandatory)][string]$Unpacked,
    [Parameter(Mandatory)][string]$Witchy,
    [Parameter(Mandatory)][string]$EsdTool,
    [Parameter(Mandatory)][string]$Smithbox,
    [Parameter(Mandatory)][string]$Me3Dir
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$out = Join-Path $repoRoot 'dist'
New-Item -ItemType Directory -Force -Path $out | Out-Null

# The game files build-gamedata.ps1 reads (it checks each pinned hash again when CI builds).
$gameFiles = @(
    'event/m10_00_00_00.emevd.dcx', 'event/common.emevd.dcx', 'event/common_func.emevd.dcx', 'event/m18_00_00_00.emevd.dcx',
    'map/mapstudio/m60_00_00_99.msb.dcx', 'regulation.bin', 'menu/05_000_title.gfx', 'script/talk/m10_00_00_00.talkesdbnd.dcx'
)
$langs = @(Get-ChildItem -LiteralPath (Join-Path $Unpacked 'msg') -Directory | Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'menu_dlc01.msgbnd.dcx') })
if ($langs.Count -ne 15) { throw "Expected 15 languages under $Unpacked\msg, found $($langs.Count)." }
$gameFiles += $langs | ForEach-Object { "msg/$($_.Name)/menu_dlc01.msgbnd.dcx" }
# Smithbox: only the DLLs sbwriter references (build-gamedata.ps1 lists the same ones), not the 700 MB install.
$smithboxDlls = 'Andre.SoulsFormats.dll', 'Andre.Core.dll', 'Andre.IO.dll', 'DotNext.dll', 'DotNext.IO.dll', 'DotNext.Unsafe.dll',
    'CommunityToolkit.HighPerformance.dll', 'ZstdNet.dll', 'libzstd.dll', 'oo2core_9_win64.dll'
$witchyDir = Split-Path (Resolve-Path -LiteralPath $Witchy).Path -Parent
$esdDir = Split-Path (Resolve-Path -LiteralPath $EsdTool).Path -Parent
if (-not (Test-Path -LiteralPath (Join-Path $esdDir 'dist'))) { throw "ESDLang's dist folder must sit beside esdtool.exe ($esdDir)." }

$pins = Get-Content -LiteralPath (Join-Path $repoRoot 'pins/nightreign.toml') -Raw
foreach ($file in 'me3.exe', 'me3-launcher.exe', 'me3_mod_host.dll') {
    $expected = [regex]::Match($pins, '"' + [regex]::Escape($file) + '"\s*=\s*"([0-9A-Fa-f]{64})"').Groups[1].Value
    if (-not $expected -or (Get-FileHash -LiteralPath (Join-Path $Me3Dir "bin/$file")).Hash -ne $expected) { throw "me3 pin mismatch: $file" }
}

$stage = Join-Path $out ('ci-inputs-stage-' + [guid]::NewGuid().ToString('N'))
foreach ($rel in $gameFiles) {
    $src = Join-Path $Unpacked $rel
    if (-not (Test-Path -LiteralPath $src)) { throw "Game file missing: $src" }
    $dst = Join-Path $stage "unpacked/$rel"
    New-Item -ItemType Directory -Force (Split-Path $dst -Parent) | Out-Null
    Copy-Item -LiteralPath $src -Destination $dst
}
New-Item -ItemType Directory -Force (Join-Path $stage 'tools/Smithbox'), (Join-Path $stage 'me3/bin') | Out-Null
Copy-Item -LiteralPath $witchyDir -Destination (Join-Path $stage 'tools/WitchyBND') -Recurse
Copy-Item -LiteralPath $esdDir -Destination (Join-Path $stage 'tools/ESDLang') -Recurse
foreach ($dll in $smithboxDlls) {
    $src = Join-Path $Smithbox $dll
    if (-not (Test-Path -LiteralPath $src)) { throw "Smithbox DLL missing: $src" }
    Copy-Item -LiteralPath $src -Destination (Join-Path $stage 'tools/Smithbox')
}
foreach ($file in 'me3.exe', 'me3-launcher.exe', 'me3_mod_host.dll') { Copy-Item -LiteralPath (Join-Path $Me3Dir "bin/$file") -Destination (Join-Path $stage 'me3/bin') }
foreach ($file in 'LICENSE-MIT', 'LICENSE-APACHE') { Copy-Item -LiteralPath (Join-Path $Me3Dir $file) -Destination (Join-Path $stage 'me3') }

$zip = Join-Path $out 'ci-inputs.zip'
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
$sha = (Get-FileHash -LiteralPath $zip).Hash
Set-Content -LiteralPath (Join-Path $out 'ci-inputs.sha256') -Value $sha -Encoding ascii
Write-Output "OK: $zip ($([math]::Round((Get-Item $zip).Length / 1MB, 1)) MB, $($gameFiles.Count) game files)"
Write-Output "Upload it to a draft release in this repository. Keep it a draft."
Write-Output "Update pins/ci-inputs.json (schema 2) with the draft's release ID and SHA-256: $sha"
# Only remove the unique staging directory we just created, after validating its absolute parent.
if ([IO.Path]::GetFullPath((Split-Path $stage -Parent)) -ne [IO.Path]::GetFullPath($out)) { throw 'Unexpected staging directory.' }
Remove-Item -LiteralPath $stage -Recurse -Force
