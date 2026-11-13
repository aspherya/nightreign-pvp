# Portable entry point for the root .bat files. No machine-specific drive letters.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Launcher', 'Mod', 'All', 'GameData', 'Release', 'CIInputs')][string]$Task,
    [string]$Version,
    [string]$NotesFile,
    [string]$Me3Dir,
    [string]$Unpacked,
    [string]$Witchy,
    [string]$Smithbox,
    [string]$EsdTool,
    [string]$Python,
    [string]$GameData,
    [ValidateRange(2, 6)][int]$MatchPlayers = 6,
    [ValidateRange(0, 6)][int]$MinPlayers = 2,
    [switch]$NonInteractive,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$configPath = Join-Path $repoRoot 'build.local.psd1'
$config = @{}
if (Test-Path -LiteralPath $configPath) { $config = Import-PowerShellDataFile -LiteralPath $configPath }

function Resolve-LocalPath([string]$value) {
    $value = [Environment]::ExpandEnvironmentVariables($value.Trim().Trim('"'))
    if (-not [IO.Path]::IsPathRooted($value)) { $value = Join-Path $repoRoot $value }
    [IO.Path]::GetFullPath($value)
}

function Save-LocalConfig {
    $lines = @('# Local build-tool locations. Gitignored; do not put credentials here.', '@{')
    foreach ($key in @($config.Keys | Sort-Object)) {
        $lines += "    $key = '$(([string]$config[$key]).Replace("'", "''"))'"
    }
    $lines += '}'
    [IO.File]::WriteAllLines($configPath, $lines, (New-Object Text.UTF8Encoding($false)))
}

function Find-ToolPath([string]$key, [string]$supplied, [string]$description, [string[]]$candidates) {
    $choices = @($supplied, $config[$key], [Environment]::GetEnvironmentVariable("NRPVP_$($key.ToUpperInvariant())")) + $candidates
    foreach ($choice in $choices) {
        if (-not $choice) { continue }
        $path = Resolve-LocalPath $choice
        if (Test-Path -LiteralPath $path) { return $path }
        # Explicit settings should fail clearly instead of silently using another installation.
        if ($choice -eq $supplied -or $choice -eq $config[$key]) { throw "$description not found: $path. Update -$key or build.local.psd1." }
    }
    if ($NonInteractive -or $CheckOnly) { throw "$description is not configured. Pass -$key <path> or edit build.local.psd1 (see build.local.example.psd1)." }
    $answer = Read-Host "$description (paste its full path)"
    if (-not $answer) { throw "$description is required." }
    $path = Resolve-LocalPath $answer
    if (-not (Test-Path -LiteralPath $path)) { throw "Path not found: $path" }
    $config[$key] = $path
    Save-LocalConfig
    return $path
}

function Initialize-Rust {
    $cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
    $script:cargo = if ($cargoCommand) { $cargoCommand.Source } else { $null }
    foreach ($candidate in @((Join-Path $env:USERPROFILE '.cargo/bin/cargo.exe'), $(if ($env:CARGO_HOME) { Join-Path $env:CARGO_HOME 'bin/cargo.exe' }))) {
        if (-not $script:cargo -and $candidate -and (Test-Path -LiteralPath $candidate)) { $script:cargo = $candidate }
    }
    if (-not $script:cargo) { throw 'Rust is missing. Install the Windows x64 MSVC toolchain from https://rustup.rs, then run this file again.' }
    $env:PATH = (Split-Path $script:cargo -Parent) + ';' + $env:PATH
    $vswhereCommand = Get-Command vswhere.exe -ErrorAction SilentlyContinue
    $vswhere = if ($vswhereCommand) { $vswhereCommand.Source } else { Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe' }
    if (Test-Path -LiteralPath $vswhere) {
        $installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($LASTEXITCODE -ne 0) { throw 'Could not query Visual Studio Build Tools.' }
        if ($installation) {
            $devCommand = Join-Path $installation 'Common7/Tools/VsDevCmd.bat'
            $environmentLines = & $env:ComSpec /d /s /c "`"`"$devCommand`" -no_logo -arch=x64 -host_arch=x64 >nul && set`""
            if ($LASTEXITCODE -ne 0) { throw 'Visual Studio could not initialize its x64 build environment.' }
            foreach ($line in $environmentLines) {
                if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
            }
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw 'MSVC C++ tools are missing. Install Visual Studio Build Tools with Desktop development with C++ and a Windows SDK.'
    }
    Write-Host "Rust: $script:cargo"
    Write-Host 'MSVC x64 build tools detected.'
}

function Invoke-Cargo([string[]]$CargoArguments) {
    & $script:cargo @CargoArguments
    if ($LASTEXITCODE -ne 0) { throw "Cargo failed (exit $LASTEXITCODE). Close any running binaries from target before rebuilding." }
}

function Copy-BuildOutput([string]$source, [string]$name) {
    $destination = Join-Path $repoRoot 'dist'
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot $source) -Destination (Join-Path $destination $name) -Force
    Write-Host "Built: $(Join-Path $destination $name)"
}

Push-Location $repoRoot
try {
    if ($Task -in 'Launcher', 'Mod', 'All', 'Release') { Initialize-Rust }
    if ($Task -in 'Release', 'CIInputs') {
        $me3Command = Get-Command me3.exe -ErrorAction SilentlyContinue
        $me3Candidates = @((Join-Path $repoRoot 'tools/local/me3'), (Join-Path $repoRoot 'dist/ci-inputs/me3'))
        if ($me3Command) { $me3Candidates += Split-Path (Split-Path $me3Command.Source -Parent) -Parent }
        $Me3Dir = Find-ToolPath 'Me3Dir' $Me3Dir 'me3 folder (containing bin and LICENSE-MIT)' $me3Candidates
        foreach ($required in 'bin/me3.exe', 'bin/me3-launcher.exe', 'bin/me3_mod_host.dll', 'LICENSE-MIT', 'LICENSE-APACHE') {
            if (-not (Test-Path -LiteralPath (Join-Path $Me3Dir $required))) { throw "me3 is incomplete: missing $required in $Me3Dir" }
        }
        if ($Task -eq 'Release') {
            if (-not $GameData) { $GameData = Join-Path $repoRoot 'dist/gamedata' }
            $GameData = Resolve-LocalPath $GameData
            if ($GameData -ne (Resolve-LocalPath 'dist/gamedata') -or -not (Test-Path -LiteralPath (Join-Path $GameData 'script/talk/m10_00_00_00.talkesdbnd.dcx'))) {
                throw 'Game data is missing or incomplete. Run build-gamedata.bat first (it writes dist/gamedata).'
            }
        }
    }
    if ($Task -in 'GameData', 'CIInputs') {
        $Unpacked = Find-ToolPath 'Unpacked' $Unpacked 'Unpacked pinned game folder (1.03.3)' @((Join-Path $repoRoot 'tools/local/unpacked'))
        $Witchy = Find-ToolPath 'Witchy' $Witchy 'WitchyBND.exe' @((Join-Path $repoRoot 'tools/local/WitchyBND/WitchyBND.exe'))
        $Smithbox = Find-ToolPath 'Smithbox' $Smithbox 'Smithbox 2.2.6 folder' @((Join-Path $repoRoot 'tools/local/Smithbox'))
        $EsdTool = Find-ToolPath 'EsdTool' $EsdTool 'ESDLang 0.5.1 esdtool.exe' @((Join-Path $repoRoot 'tools/local/ESDLang/esdtool.exe'))
        $pythonCandidates = @()
        foreach ($name in 'python.exe', 'py.exe') {
            $command = Get-Command $name -ErrorAction SilentlyContinue
            if ($command -and $command.Source -notmatch '\\WindowsApps\\') { $pythonCandidates += $command.Source }
        }
        $Python = Find-ToolPath 'Python' $Python 'Python 3 executable (python.exe or py.exe)' $pythonCandidates
        $dotnetCommand = Get-Command dotnet.exe -ErrorAction SilentlyContinue
        if (-not $dotnetCommand -and (Test-Path -LiteralPath (Join-Path $env:ProgramFiles 'dotnet/dotnet.exe'))) {
            $env:PATH = (Join-Path $env:ProgramFiles 'dotnet') + ';' + $env:PATH
            $dotnetCommand = Get-Command dotnet.exe -ErrorAction SilentlyContinue
        }
        if ($Task -eq 'GameData' -and (-not $dotnetCommand -or -not (& $dotnetCommand.Source --list-sdks | Where-Object { $_ -match '^10\.' }))) {
            throw 'Game-data generation requires the .NET 10 SDK. Install it from https://dotnet.microsoft.com/download/dotnet/10.0.'
        }
    }
    if ($Task -eq 'Release') {
        if (-not $Version -and -not $NonInteractive -and -not $CheckOnly) { $Version = Read-Host 'Release version (for example 0.3.0)' }
        if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Pass -Version major.minor.patch when building a release.' }
        if (-not $NotesFile) { $NotesFile = Join-Path $repoRoot 'releases/notes.txt' }
        $NotesFile = Resolve-LocalPath $NotesFile
        if (-not (Test-Path -LiteralPath $NotesFile)) { throw "Release notes not found: $NotesFile" }
    }
    if ($CheckOnly) { Write-Host "Prerequisites detected for $Task; no build was started."; exit 0 }
    switch ($Task) {
        'Launcher' {
            Invoke-Cargo -CargoArguments @('build', '--locked', '--release', '-p', 'nrpvp-app', '-p', 'nrpvp-launcher')
            Copy-BuildOutput 'target/release/NightreignPvP.exe' 'NightreignPvP-Launcher.exe'
            Copy-BuildOutput 'target/release/nrpvp_launcher.exe' 'nrpvp_launcher.exe'
        }
        'Mod' {
            $arguments = @('build', '--locked', '--release', '-p', 'nrpvp', '--no-default-features', '--target-dir', (Join-Path $repoRoot 'target/player'))
            Invoke-Cargo -CargoArguments $arguments
            Copy-BuildOutput 'target/player/release/nrpvp.dll' 'nrpvp.dll'
        }
        'All' {
            Invoke-Cargo -CargoArguments @('build', '--locked', '--release', '--workspace')
            foreach ($file in 'NightreignPvP.exe', 'nrpvp_launcher.exe', 'nrpvp.dll', 'nrcon.exe', 'nro.exe', 'nro-mcp.exe') {
                Copy-BuildOutput "target/release/$file" $file
            }
        }
        'GameData' {
            & (Join-Path $PSScriptRoot 'build-gamedata.ps1') -Unpacked $Unpacked -Witchy $Witchy -Smithbox $Smithbox -EsdTool $EsdTool -Python $Python
        }
        'Release' {
            & (Join-Path $PSScriptRoot 'publish-release.ps1') -Version $Version -NotesFile $NotesFile -Me3Dir $Me3Dir -MatchPlayers $MatchPlayers -MinPlayers $MinPlayers -DryRun -AllowDirty
        }
        'CIInputs' {
            & (Join-Path $PSScriptRoot 'prepare-ci-inputs.ps1') -Unpacked $Unpacked -Witchy $Witchy -Smithbox $Smithbox -EsdTool $EsdTool -Me3Dir $Me3Dir
        }
    }
    Write-Host 'Build completed successfully.' -ForegroundColor Green
}
catch {
    Write-Host "Build failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
finally { Pop-Location }
