[CmdletBinding()]
param(
    [ValidateSet('auto', 'x64', 'arm64')][string]$Architecture = 'auto',
    [ValidateSet('workspace', 'codec', 'sdk')][string]$Package = 'workspace',
    [switch]$DebugBuild,
    [switch]$Test,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
    throw 'Use scripts/build.sh on Ubuntu 24.04.'
}
$hostArch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($Architecture -eq 'auto') { $Architecture = $hostArch }
if ($hostArch -notin @('x64', 'arm64')) { throw "Unsupported host architecture: $hostArch" }
if ($Test -and $Architecture -ne $hostArch) {
    throw 'Run tests on the native target architecture. Cross-compilation is build-only.'
}
$target = @{x64='x86_64-pc-windows-msvc'; arm64='aarch64-pc-windows-msvc'}[$Architecture]
$root = Split-Path $PSScriptRoot -Parent

# Import the selected MSVC environment without printing its environment variables.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio Build Tools with C++ tools and a Windows SDK.' }
$toolsetComponent = switch ($Architecture) {
    'x64' { 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64' }
    'arm64' { 'Microsoft.VisualStudio.Component.VC.Tools.ARM64' }
}
$vs = & $vswhere -latest -products '*' -requires $toolsetComponent -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $vs) {
    throw "No Visual Studio installation contains the required $Architecture C++ tools ($toolsetComponent)."
}
$vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvarsall.bat'
if (-not (Test-Path -LiteralPath $vcvars)) { throw 'Install the Visual Studio C++ workload.' }
$vcArch = if ($hostArch -eq 'arm64') {
    if ($Architecture -eq 'arm64') { 'arm64' } else { 'arm64_x64' }
} else {
    if ($Architecture -eq 'arm64') { 'x64_arm64' } else { 'x64' }
}
$variables = & $env:ComSpec /d /c "call `"$vcvars`" $vcArch >nul && set"
if ($LASTEXITCODE -ne 0) { throw "Cannot initialize MSVC for $vcArch. Install the corresponding C++ tools." }
foreach ($line in $variables) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}

function Add-ToolPath([string]$Directory) {
    if (Test-Path -LiteralPath $Directory -PathType Container) { $env:PATH = "$Directory;$env:PATH" }
}
if ($env:CARGO_HOME) { Add-ToolPath (Join-Path $env:CARGO_HOME 'bin') }
if ($Package -ne 'codec') {
    Add-ToolPath (Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin')
    Add-ToolPath (Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja')
    Add-ToolPath (Join-Path $env:ProgramFiles 'LLVM\bin')
    Add-ToolPath (Join-Path $env:ProgramFiles 'NASM')
    # Append Git commands and Unix utilities, keeping the MSVC linker first.
    foreach ($gitFolder in @('Git\cmd', 'Git\usr\bin')) {
        $gitDirectory = Join-Path $env:ProgramFiles $gitFolder
        if (Test-Path -LiteralPath $gitDirectory) { $env:PATH += ";$gitDirectory" }
    }
    $env:CMAKE_GENERATOR = 'Ninja'
    foreach ($tool in @('cmake', 'ninja', 'perl', 'git')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing SDK prerequisite: $tool" }
    }
    if ($Architecture -eq 'x64' -and -not (Get-Command nasm -ErrorAction SilentlyContinue)) {
        throw 'Windows x64 SDK builds require NASM on PATH.'
    }
    if (-not $env:LIBCLANG_PATH) {
        $llvm = Join-Path $env:ProgramFiles 'LLVM\bin'
        if (Test-Path -LiteralPath (Join-Path $llvm 'libclang.dll')) { $env:LIBCLANG_PATH = $llvm }
    }
    if (-not $env:LIBCLANG_PATH -or -not (Test-Path -LiteralPath (Join-Path $env:LIBCLANG_PATH 'libclang.dll'))) {
        throw 'Install host-native LLVM and set LIBCLANG_PATH to the folder containing libclang.dll.'
    }
}
foreach ($tool in @('cargo', 'rustc')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing Rust prerequisite: $tool" }
}

Push-Location $root
try {
    $selection = switch ($Package) {
        'codec' { @('-p', 'akamai-codec') }
        'sdk' { @('-p', 'akamai-solver') }
        'workspace' { @('--workspace') }
    }
    $common = @('--locked', '--target', $target) + $selection
    if (-not $DebugBuild) { $common += '--release' }
    if ($Offline) { $common += '--offline' }
    Write-Host "Building $Package for $target from $root"
    & cargo build @common --all-targets
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE)." }
    if ($Test) {
        & cargo test @common --lib --bins
        if ($LASTEXITCODE -ne 0) { throw "Cargo tests failed ($LASTEXITCODE)." }
    }
} finally { Pop-Location }
