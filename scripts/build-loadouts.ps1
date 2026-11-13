# Build the training equipment dialogue from the pinned original, preserving every other ESD byte-for-byte.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Unpacked,
    [Parameter(Mandatory)][string]$Witchy,
    [Parameter(Mandatory)][string]$EsdTool,
    [Parameter(Mandatory)][string]$Python,
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$original = Join-Path $Unpacked 'script/talk/m10_00_00_00.talkesdbnd.dcx'
if ((Get-FileHash -LiteralPath $original).Hash -ne '43D5005F1934A37121CA2589C58684E73964F2440B1D13BB79443AC689796CD1') {
    throw 'Training dialogue bundle is not the pinned NR 1.03.3 file.'
}
$EsdTool = (Resolve-Path -LiteralPath $EsdTool).Path
$Witchy = (Resolve-Path -LiteralPath $Witchy).Path
$Unpacked = (Resolve-Path -LiteralPath $Unpacked).Path
$Output = [IO.Path]::GetFullPath($Output)
$work = Join-Path $repo ('target/loadout-build/' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $work | Out-Null
$bundle = Join-Path $work 'm10_00_00_00.talkesdbnd.dcx'
Copy-Item -LiteralPath $original -Destination $bundle
& $Witchy -s -p $bundle
if ($LASTEXITCODE -ne 0) { throw 'Training dialogue unpack failed.' }
$esds = Join-Path $work 'm10_00_00_00-talkesdbnd-dcx'
$hashes = @{}
Get-ChildItem -LiteralPath $esds -Filter '*.esd' | ForEach-Object { $hashes[$_.Name] = (Get-FileHash -LiteralPath $_.FullName).Hash }
$source = Join-Path $work 'original/t607001000.py'
$edited = Join-Path $work 't607001000.py'
$compiled = Join-Path $work 't607001000.esd'
function Invoke-Esd([string[]]$Arguments) {
    Push-Location (Split-Path -Parent $EsdTool)
    try {
        # Disable annotations: the compiler needs no regulation/menu reads. Read loose ESDs so Witchy alone handles compression.
        $log = & $EsdTool -nr -noannotate -basedir $Unpacked @Arguments 2>&1
        $code = $LASTEXITCODE
        if ($code -ne 0 -or ($log -match 'ERROR|Error processing|Exception|Error:')) { $log | Out-Host; throw 'esdtool failed.' }
    } finally { Pop-Location }
}
Invoke-Esd @('-i', (Join-Path $esds 't607001000.esd'), '-writepy', $source)
if (-not (Test-Path -LiteralPath $source)) { throw 'esdtool did not decompile the training interaction.' }
& $Python -I (Join-Path $repo 'tools/loadouts/patch_talk.py') $source $edited
if ($LASTEXITCODE -ne 0) { throw 'Training dialogue patch failed.' }
Invoke-Esd @('-esddir', $esds, '-f', 't607001000', '-i', $edited, '-writeloose', $compiled)
if (-not (Test-Path -LiteralPath $compiled)) { throw 'esdtool did not compile the saved loadout menu.' }
Copy-Item -LiteralPath $compiled -Destination (Join-Path $esds 't607001000.esd')
foreach ($name in $hashes.Keys) {
    if ($name -ne 't607001000.esd' -and (Get-FileHash -LiteralPath (Join-Path $esds $name)).Hash -ne $hashes[$name]) {
        throw "Unexpected change to training bundle entry $name"
    }
}
& $Witchy -s -p $esds
if ($LASTEXITCODE -ne 0) { throw 'Training dialogue repack failed.' }
New-Item -ItemType Directory -Force (Split-Path -Parent $Output) | Out-Null
Copy-Item -LiteralPath $bundle -Destination $Output
Write-Host "Saved loadouts: training interaction built ($($hashes.Count - 1) other scripts preserved)."
