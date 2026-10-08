param(
    [switch]$Debug,
    [switch]$NoBundle,
    [switch]$Production
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-env.ps1')
. (Join-Path $PSScriptRoot 'build-lock.ps1')
. (Join-Path $PSScriptRoot 'fast-build-env.ps1')
$cmProjectDirectory = Split-Path -Parent $PSScriptRoot
$cmBuildLock = $null
$cmPreviousProfile = $null
Push-Location $cmProjectDirectory
try {
    $cmBuildLock = Enter-CmBuildLock -ProjectDirectory $cmProjectDirectory
    if (-not $Debug) { $cmPreviousProfile = Enter-CmReleaseProfile -Production:$Production }
    $cmBuildArguments = @('run', 'tauri', '--', 'build', '--features', 'custom-protocol')
    if ($Debug) { $cmBuildArguments += '--debug' }
    if ($NoBundle) { $cmBuildArguments += '--no-bundle' }
    & npm @cmBuildArguments
    if ($LASTEXITCODE -ne 0) { throw "Windows build exited with code $LASTEXITCODE" }
} finally {
    if ($cmBuildLock) { $cmBuildLock.Dispose() }
    if ($cmPreviousProfile) { Exit-CmReleaseProfile -Previous $cmPreviousProfile }
    Pop-Location
}
