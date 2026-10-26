# Build only the developer mod (nrpvp.dll + nrcon.exe) and copy them to dist\. Unlike build.ps1 it leaves nro.exe alone, so it works
# while `nro desk` is running (a running desk locks target\release\nro.exe and a full `cargo build --release` then fails).
# Usage: ./scripts/build-mod.ps1
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) { $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
Push-Location $repo
try {
    & $cargo build --release -p nrpvp -p nrcon
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)" }
    $dist = Join-Path $repo 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item (Join-Path $repo 'target\release\nrpvp.dll') (Join-Path $dist 'nrpvp.dll') -Force
    $pdb = Join-Path $repo 'target\release\nrpvp.pdb'
    if (Test-Path $pdb) { Copy-Item $pdb (Join-Path $dist 'nrpvp.pdb') -Force }
    Copy-Item (Join-Path $repo 'target\release\nrcon.exe') (Join-Path $dist 'nrcon.exe') -Force
    Write-Host "OK: $(Join-Path $dist 'nrpvp.dll')"
}
finally { Pop-Location }
