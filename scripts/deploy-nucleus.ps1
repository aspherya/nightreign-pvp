# Deploys dist\nrpvp.dll into the Nucleus Co-op handler for ELDEN RING NIGHTREIGN (FS-0: local dual-instance tests).
# Nucleus copies the handler folder into every Instance<N>\ on launch; its loader (coldloader.dll) loads each DLL named in
# dlllist.txt, so nrpvp.dll ends up in every instance and writes its own nrpvp.log / logs\ in that instance folder.
# Use -Remove to undo. Run with Nucleus instances closed (the DLL is copied on Nucleus launch, not live).
param(
    [string]$HandlerDir = 'D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN',
    [int]$OverlayDelayMs = 60000,   # FS-0: let Nucleus ProtoInput inject before nrpvp hooks D3D12
    [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$list = Join-Path $HandlerDir 'dlllist.txt'
if (-not (Test-Path $list)) { throw "dlllist.txt not found in $HandlerDir" }

$backup = "$list.nrpvp-backup"
if (-not (Test-Path $backup)) { Copy-Item $list $backup }

$lines = @(Get-Content $list | Where-Object { $_.Trim() -ne '' -and $_.Trim() -ne 'nrpvp.dll' })
if ($Remove) {
    [IO.File]::WriteAllText($list, ($lines -join "`r`n"))  # no trailing newline: coldloader treats an empty last line as a DLL name
    Remove-Item (Join-Path $HandlerDir 'nrpvp.dll'), (Join-Path $HandlerDir 'nrpvp.pdb'), (Join-Path $HandlerDir 'nrpvp.toml') -ErrorAction SilentlyContinue
    Write-Output "Removed nrpvp from $HandlerDir"
    return
}

$dll = Join-Path $repo 'dist\nrpvp.dll'
if (-not (Test-Path $dll)) { throw "Build first: $dll missing (scripts\build.ps1)" }
Copy-Item $dll $HandlerDir -Force
Copy-Item (Join-Path $repo 'dist\nrpvp.pdb') $HandlerDir -Force -ErrorAction SilentlyContinue
$toml = Join-Path $repo 'dist\nrpvp.toml'
if (Test-Path $toml) { Copy-Item $toml $HandlerDir -Force }
$htoml = Join-Path $HandlerDir 'nrpvp.toml'
$body = @(if (Test-Path $htoml) { Get-Content $htoml | Where-Object { $_ -notmatch '^\s*(overlay_install_delay_ms|log_dir)\s*=' } })
# T-018: logs go outside Nucleus's Instance<N> folders (deleted on close). TOML single-quoted literal = no escapes.
$logDir = Join-Path $repo 'dist\nucleus-logs'
New-Item -ItemType Directory -Force $logDir | Out-Null
Set-Content -Path $htoml -Value ($body + "overlay_install_delay_ms = $OverlayDelayMs" + "log_dir = '$logDir'") -Encoding Ascii
[IO.File]::WriteAllText($list, (($lines + 'nrpvp.dll') -join "`r`n"))  # no trailing newline (see above)
Write-Output "Deployed nrpvp.dll ($((Get-Item $dll).LastWriteTime)) to $HandlerDir"
Get-Content $list
