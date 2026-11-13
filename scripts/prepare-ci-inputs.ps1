# Run after build-gamedata.ps1 on a machine with the pinned game unpack.
# Upload dist/ci-inputs.zip to a DRAFT release in this repository tagged build-inputs-v1.
param(
    [Parameter(Mandatory)][string]$GameData,
    [Parameter(Mandatory)][string]$Me3Dir
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$out = Join-Path $repoRoot 'dist'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$resolvedData = (Resolve-Path -LiteralPath $GameData).Path
if ((Split-Path $resolvedData -Leaf) -ne 'gamedata-player') {
    throw 'Use the gamedata-player output of build-gamedata.ps1. Do not use the installed Arena test package.'
}
if (@(Get-ChildItem -LiteralPath $resolvedData -Recurse -File).Count -ne 21 -or -not (Test-Path -LiteralPath (Join-Path $resolvedData 'menu/05_000_title.gfx'))) { throw 'Expected the 21-file player game-data package, including the custom title background.' }
$pins = Get-Content -LiteralPath (Join-Path $repoRoot 'pins/nightreign.toml') -Raw
foreach ($file in 'me3.exe', 'me3-launcher.exe', 'me3_mod_host.dll') {
    $expected = [regex]::Match($pins, '"' + [regex]::Escape($file) + '"\s*=\s*"([0-9A-Fa-f]{64})"').Groups[1].Value
    if (-not $expected -or (Get-FileHash -LiteralPath (Join-Path $Me3Dir "bin/$file")).Hash -ne $expected) { throw "me3 pin mismatch: $file" }
}
$stage = Join-Path $out ('ci-inputs-stage-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $stage 'me3/bin') -Force | Out-Null
Copy-Item -LiteralPath $resolvedData -Destination (Join-Path $stage 'gamedata-player') -Recurse
foreach ($file in 'me3.exe', 'me3-launcher.exe', 'me3_mod_host.dll') { Copy-Item -LiteralPath (Join-Path $Me3Dir "bin/$file") -Destination (Join-Path $stage 'me3/bin') }
foreach ($file in 'LICENSE-MIT', 'LICENSE-APACHE') { Copy-Item -LiteralPath (Join-Path $Me3Dir $file) -Destination (Join-Path $stage 'me3') }
$zip = Join-Path $out 'ci-inputs.zip'
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
$sha = (Get-FileHash -LiteralPath $zip).Hash
Set-Content -LiteralPath (Join-Path $out 'ci-inputs.sha256') -Value $sha -Encoding ascii
Write-Output "Upload $zip to a draft release in this repository tagged build-inputs-v1. Keep it a draft."
Write-Output "Update pins/ci-inputs.json with the returned draft release ID and SHA-256: $sha"
# Only remove the unique staging directory we just created, after validating its absolute parent.
if ([IO.Path]::GetFullPath((Split-Path $stage -Parent)) -ne [IO.Path]::GetFullPath($out)) { throw 'Unexpected staging directory.' }
Remove-Item -LiteralPath $stage -Recurse -Force
