# Runs every scenarios\*.toml (or the names given) through nro against dist\ and prints one verdict line each.
# Needs `nro desk` running in a normal terminal and Steam running. Usage: scripts\regress.ps1 [night1-count night2-win ...]
# Multi-instance scenarios (mi-*: emulated Steam, Steam closed) only run when named.
param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Names)
$repo = Split-Path $PSScriptRoot -Parent
$nro = Join-Path $repo 'target\release\nro.exe'
if (-not (Test-Path $nro)) { throw "build nro first: cargo build --release -p nro -p nro-mcp" }
if (-not $Names) { $Names = Get-ChildItem (Join-Path $repo 'scenarios') -Filter *.toml | Where-Object { $_.BaseName -notlike 'mi-*' } | ForEach-Object { $_.BaseName } }
$bad = 0
foreach ($n in $Names) {
    & $nro stop *> $null
    Start-Sleep -Seconds 6
    $json = & $nro run (Join-Path $repo "scenarios\$n.toml") | Out-String
    $v = ([regex]::Match($json, '"verdict":\s*"(\w+)"')).Groups[1].Value
    $run = ([regex]::Match($json, '"run":\s*"([^"]+)"')).Groups[1].Value
    Write-Host ("{0,-22} {1}  {2}" -f $n, $v, $run)
    if ($v -ne 'PASS') { $bad++ }
}
& $nro stop *> $null
exit $bad
