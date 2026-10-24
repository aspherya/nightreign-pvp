# Deploys dist\nrpvp.dll into the Nucleus Co-op handler for ELDEN RING NIGHTREIGN (FS-0: local dual-instance tests).
# Nucleus copies the handler folder into every Instance<N>\ on launch; its loader (coldloader.dll) loads each DLL named in
# dlllist.txt, so nrpvp.dll ends up in every instance and writes its own nrpvp.log / logs\ in that instance folder.
# SECONDARY TOOL (2026-10-24): quick two-window checks of plain rules (downed rule, teams, revive, matchmaking-free play). It starts
# the game through Seamless Co-op's launcher, so our game-data package (event files: nights, the Roundtable menu, the hawk line) is NOT
# loaded and the console clock / night commands are the stand-ins. Anything that needs the event files goes through the orchestrator
# (`nro`, offline, puppets) or a friend session. The old -Me3 mode (start through me3) failed for window 2 and was removed.
# Use -Remove to undo. Run with Nucleus instances closed (the DLL is copied on Nucleus launch, not live).
param(
    [string]$HandlerDir = 'D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN',
    [bool]$Pvpve = $true,           # T-020: PvPvE mode on in every instance (writes pvpve = true; -Pvpve:$false = off). Console: pvp 0 / pvp 1
    [ValidateRange(1,3)][int]$TeamSize = 1,   # T-026: players per team (1 = free-for-all)
    [ValidateSet('rivals','all')][string]$Scheme = 'rivals',   # T-026: write the hostile team value only to rivals, or to everyone (T-020)
    [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$list = Join-Path $HandlerDir 'dlllist.txt'
if (-not (Test-Path $list)) { throw "dlllist.txt not found in $HandlerDir" }

$backup = "$list.nrpvp-backup"
if (-not (Test-Path $backup)) { Copy-Item $list $backup }

$js = Join-Path (Split-Path $HandlerDir -Parent) ((Split-Path $HandlerDir -Leaf) + '.js')
function Set-NucleusLauncher([string]$exe) {
    if (-not (Test-Path $js)) { throw "Nucleus handler script not found: $js" }
    if (-not (Test-Path "$js.nrpvp-backup")) { Copy-Item $js "$js.nrpvp-backup" }
    $t = [IO.File]::ReadAllText($js)
    $t2 = [regex]::Replace($t, 'Game\.LauncherExe = "[^"]*";', "Game.LauncherExe = `"$exe`";")
    if ($t2 -ne $t) { [IO.File]::WriteAllText($js, $t2); Write-Output "Nucleus handler: LauncherExe = $exe" }
}
function Remove-Me3Layout {
    Remove-Item (Join-Path $HandlerDir 'NightreignPvP') -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item (Join-Path $HandlerDir 'nrpvp_launcher.exe') -Force -ErrorAction SilentlyContinue
}
$lines = @(Get-Content $list | Where-Object { $_.Trim() -ne '' -and $_.Trim() -ne 'nrpvp.dll' })
if ($Remove) {
    [IO.File]::WriteAllText($list, ($lines -join "`r`n"))  # no trailing newline: coldloader treats an empty last line as a DLL name
    Remove-Item (Join-Path $HandlerDir 'nrpvp.dll'), (Join-Path $HandlerDir 'nrpvp.pdb'), (Join-Path $HandlerDir 'nrpvp.toml') -ErrorAction SilentlyContinue
    Remove-Me3Layout
    Set-NucleusLauncher 'nrsc_launcher.exe'
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
$body = @(if (Test-Path $htoml) { Get-Content $htoml | Where-Object { $_ -notmatch '^\s*(dev_mode|log_dir|auto_team_all|pvpve|pvpve_team|pvpve_scheme|team_size|probes|probe_interval_ms|overlay_\w+|hotkey_\w+|downed_rival_\w+|ui_c1_proto|mm_queue|br_menu)\s*=' } })
# Nucleus runs the instances over Goldberg (no real Steam lobbies): our queue and the table-menu takeover stay off, so instances meet
# through Seamless Co-op's normal Invite / Join flow and Commence starts a normal expedition.
# T-018: logs go outside Nucleus's Instance<N> folders (deleted on close). TOML single-quoted literal = no escapes.
$logDir = Join-Path $repo 'dist\nucleus-logs'
New-Item -ItemType Directory -Force $logDir | Out-Null
$keys = @("dev_mode = true", "log_dir = '$logDir'", "pvpve = $($Pvpve.ToString().ToLower())", "pvpve_scheme = '$Scheme'", "team_size = $TeamSize", "mm_queue = false", "br_menu = false")
# HAWK-2: top-level keys must come before the first [table] line (e.g. [hawk]); after it they would belong to that table.
$firstTable = @(0..($body.Count) | Where-Object { $_ -lt $body.Count -and $body[$_] -match '^\s*\[' } | Select-Object -First 1)
if ($firstTable.Count -eq 1) {
    $i = $firstTable[0]
    $head = if ($i -gt 0) { @($body[0..($i - 1)]) } else { @() }
    $body = $head + $keys + @($body[$i..($body.Count - 1)])
} else {
    $body = $body + $keys
}
Set-Content -Path $htoml -Value $body -Encoding Ascii
[IO.File]::WriteAllText($list, (($lines + 'nrpvp.dll') -join "`r`n"))  # no trailing newline (see above)
Write-Output "Deployed nrpvp.dll ($((Get-Item $dll).LastWriteTime)) to $HandlerDir"
Get-Content $list

# Leftovers of the removed -Me3 mode (a handler that was deployed with it): back to Seamless Co-op's launcher.
Remove-Me3Layout
Set-NucleusLauncher 'nrsc_launcher.exe'
