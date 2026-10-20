# Build nrpvp.dll (release) and copy it to dist/.
# Usage: ./scripts/build.ps1
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot

# Find cargo whether or not ~/.cargo/bin is on PATH.
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
    if (Test-Path $candidate) { $cargo = $candidate }
}
if (-not $cargo) { throw 'cargo not found on PATH or in %USERPROFILE%\.cargo\bin. Install Rust via rustup.' }

Push-Location $repo
try {
    & $cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit code $LASTEXITCODE)" }

    $src = Join-Path $repo 'target\release\nrpvp.dll'
    if (-not (Test-Path $src)) { throw "Build succeeded but $src was not produced." }

    $dist = Join-Path $repo 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item $src (Join-Path $dist 'nrpvp.dll') -Force
    # Keep the PDB beside the DLL so crash addresses can be symbolised.
    $pdb = Join-Path $repo 'target\release\nrpvp.pdb'
    if (Test-Path $pdb) { Copy-Item $pdb (Join-Path $dist 'nrpvp.pdb') -Force }
    # CON-2: the console client next to the DLL (`dist\nrcon.exe status` talks to a running developer game).
    $nrcon = Join-Path $repo 'target\release\nrcon.exe'
    if (Test-Path $nrcon) { Copy-Item $nrcon (Join-Path $dist 'nrcon.exe') -Force }

    Write-Host "OK: $(Join-Path $dist 'nrpvp.dll')"
}
finally {
    Pop-Location
}
