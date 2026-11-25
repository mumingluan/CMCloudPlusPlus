param(
    [switch]$Debug,
    [switch]$NoBundle,
    [switch]$Production
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-env.ps1')
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

    if (-not $Debug) {
        $cmVersion = (Get-Content -LiteralPath (Join-Path $cmProjectDirectory 'package.json') -Raw | ConvertFrom-Json).version
        $cmTargetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $cmProjectDirectory 'src-tauri\target' }
        $cmExe = Join-Path $cmTargetRoot 'release\cmcloud.exe'
        $cmReleaseDir = Join-Path $cmProjectDirectory 'releases'
        New-Item -ItemType Directory -Force -Path $cmReleaseDir | Out-Null
        $cmReleaseExe = Join-Path $cmReleaseDir "CMCloudPlusPlus-$cmVersion-windows-x64.exe"
        Copy-Item -LiteralPath $cmExe -Destination $cmReleaseExe -Force
        Get-Item -LiteralPath $cmReleaseExe | Select-Object FullName, Length
    }
} finally {
    if ($cmBuildLock) { $cmBuildLock.Dispose() }
    if ($cmPreviousProfile) { Exit-CmReleaseProfile -Previous $cmPreviousProfile }
    Pop-Location
}
