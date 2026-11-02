# Runs scenarios\*.toml (or the names given) through nro against dist\ and prints one verdict line each, with its seconds and a total.
# Needs `nro desk` running in a normal terminal and Steam running.
# Usage: scripts\regress.ps1 [day1-free match-win ...]     named scenarios (any tag)
#        scripts\regress.ps1 -Tag smoke                    every scenario with that tag (tags: smoke rules hud mi slow lab)
# With neither, every scenario WITHOUT the tags `mi` and `lab` runs (multi-instance ones need Steam closed; labs are not tests).
[CmdletBinding(PositionalBinding = $false)]
param([string]$Tag, [Parameter(ValueFromRemainingArguments = $true)][string[]]$Names)
$repo = Split-Path $PSScriptRoot -Parent
$nro = Join-Path $repo 'target\release\nro.exe'
if (-not (Test-Path $nro)) { throw "build nro first: cargo build --release -p nro -p nro-mcp" }
if (-not $Names) {
    $all = & $nro scenarios | Out-String | ConvertFrom-Json
    if ($Tag) {
        $sel = $all | Where-Object { $_.tags -contains $Tag }
    } else {
        $sel = $all | Where-Object { -not ($_.tags -contains 'mi') -and -not ($_.tags -contains 'lab') }
    }
    $Names = @($sel | ForEach-Object { [IO.Path]::GetFileNameWithoutExtension($_.file) })
    if (-not $Names) { throw "no scenario matches" }
}
$bad = 0
$total = [Diagnostics.Stopwatch]::StartNew()
foreach ($n in $Names) {
    & $nro stop *> $null
    Start-Sleep -Seconds 6
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $json = & $nro run (Join-Path $repo "scenarios\$n.toml") | Out-String
    $sw.Stop()
    $v = ([regex]::Match($json, '"verdict":\s*"(\w+)"')).Groups[1].Value
    $run = ([regex]::Match($json, '"run":\s*"([^"]+)"')).Groups[1].Value
    Write-Host ("{0,-22} {1,-8} {2,5:N0} s  {3}" -f $n, $v, $sw.Elapsed.TotalSeconds, $run)
    if ($v -ne 'PASS') { $bad++ }
}
& $nro stop *> $null
$total.Stop()
Write-Host ("{0} scenario(s), {1} not PASS, total {2:N0} s ({3:N1} min)" -f @($Names).Count, $bad, $total.Elapsed.TotalSeconds, $total.Elapsed.TotalMinutes)
exit $bad
