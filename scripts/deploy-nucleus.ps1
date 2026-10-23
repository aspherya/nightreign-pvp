# Deploys dist\nrpvp.dll into the Nucleus Co-op handler for ELDEN RING NIGHTREIGN (FS-0: local dual-instance tests).
# Nucleus copies the handler folder into every Instance<N>\ on launch; its loader (coldloader.dll) loads each DLL named in
# dlllist.txt, so nrpvp.dll ends up in every instance and writes its own nrpvp.log / logs\ in that instance folder.
# -Me3 (M5-8 R0): Nucleus starts nrpvp_launcher.exe instead of Seamless Co-op's launcher; it runs me3 with SC, nrpvp.dll and our
# game-data package (NightreignPvP\ in the handler, the player-zip layout), so event-script features work in two windows too.
# Use -Remove to undo. Run with Nucleus instances closed (the DLL is copied on Nucleus launch, not live).
param(
    [string]$HandlerDir = 'D:\Programs\Nucleus\handlers\ELDEN RING NIGHTREIGN',
    [bool]$Pvpve = $true,           # T-020: PvPvE mode on in every instance (writes pvpve = true; -Pvpve:$false = off). Console: pvp 0 / pvp 1
    [ValidateRange(1,3)][int]$TeamSize = 1,   # T-026: players per team (1 = free-for-all)
    [ValidateSet('rivals','all')][string]$Scheme = 'rivals',   # T-026: write the hostile team value only to rivals, or to everyone (T-020)
    [switch]$Me3,                   # M5-8 R0: start each instance through me3 (our launcher), so our game-data package (event files) loads
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

if ($Me3) {
    # M5-9 N0 (2026-10-23): me3.exe / me3-launcher.exe of a closed Nucleus run can stay alive and lock the Instance<N>\ folders, and
    # the next Nucleus start then fails ("cleanup failed", coldloader not found). Stop the ones started from Nucleus's content folder.
    Get-Process me3, me3-launcher -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -like '*\Nucleus\content\*' } |
        ForEach-Object { Write-Output "stopping leftover $($_.Name) (pid $($_.Id))"; Stop-Process -Id $_.Id -Force -Confirm:$false }
    # M5-8 R0: the player-zip layout inside the handler; Nucleus copies it into every Instance<N>\ and runs our launcher there.
    $mod = Join-Path $HandlerDir 'NightreignPvP'
    Remove-Me3Layout
    New-Item -ItemType Directory -Force (Join-Path $mod 'me3') | Out-Null
    $launcher = Join-Path $repo 'target\release\nrpvp_launcher.exe'
    if (-not (Test-Path $launcher)) { throw "Build the launcher first: cargo build --release -p nrpvp-launcher" }
    $gd = Join-Path $repo 'dist\gamedata'
    if (-not (Test-Path (Join-Path $gd 'event\common.emevd.dcx'))) { throw "Build the game-data package first: scripts\build-gamedata.ps1" }
    Copy-Item $launcher $HandlerDir -Force
    Move-Item (Join-Path $HandlerDir 'nrpvp.dll'), (Join-Path $HandlerDir 'nrpvp.toml') $mod -Force
    Move-Item (Join-Path $HandlerDir 'nrpvp.pdb') $mod -Force -ErrorAction SilentlyContinue
    Copy-Item $gd (Join-Path $mod 'gamedata') -Recurse -Force
    foreach ($f in 'me3.exe', 'me3-launcher.exe', 'me3_mod_host.dll') { Copy-Item (Join-Path 'Z:\Tools\me3\bin' $f) (Join-Path $mod 'me3') -Force }
    # Nucleus runs with Steam closed (Goldberg-style emulation inside the game): me3 must not initialise Steam itself.
    [IO.File]::WriteAllText((Join-Path $mod 'me3-args.txt'), "# deploy-nucleus.ps1 -Me3 (M5-8 R0)`r`n--skip-steam-init true`r`n")
    # M5-9 N0 (2026-10-23): Nucleus's Steam emulator (coldloader.dll) reads the app id from steam_appid.txt in the working folder;
    # a window started by me3 may run from me3's folder ("Failed to read coldloader.ini: appid not found"). Put one in both folders.
    foreach ($d in $mod, (Join-Path $mod 'me3')) { [IO.File]::WriteAllText((Join-Path $d 'steam_appid.txt'), '2622380') }
    [IO.File]::WriteAllText($list, ($lines -join "`r`n"))  # me3 loads nrpvp.dll: not coldloader (no trailing newline, see above)
    Set-NucleusLauncher 'nrpvp_launcher.exe'
    Write-Output "me3 mode: instances start through nrpvp_launcher.exe (me3: SC + nrpvp.dll + gamedata)"
    Get-Content $list
} else {
    Remove-Me3Layout
    Set-NucleusLauncher 'nrsc_launcher.exe'
}