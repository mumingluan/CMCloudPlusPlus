param(
    [switch]$Release,
    [switch]$Production,
    [ValidateSet('aarch64', 'armv7', 'x86_64', 'i686')]
    [string]$Target = 'aarch64'
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-env.ps1')
. (Join-Path $PSScriptRoot 'android-env.ps1')
. (Join-Path $PSScriptRoot 'build-lock.ps1')
. (Join-Path $PSScriptRoot 'fast-build-env.ps1')
$cmProjectDirectory = Split-Path -Parent $PSScriptRoot
$cmBuildLock = $null
$cmPreviousProfile = $null
$cmSavedEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$cmSavedReuseNative = $env:CMCLOUD_CLI_PREBUILT_NATIVE
$cmUseEncodedRustFlags = $Target -in @('aarch64', 'x86_64') -and -not $env:CARGO_ENCODED_RUSTFLAGS -and -not $env:RUSTFLAGS
if ($cmUseEncodedRustFlags) {
    # Tauri sets its own target-specific flags for the initial Cargo invocation.
    # Cargo's encoded flags keep the initial and Gradle-driven link steps equal.
    $env:CARGO_ENCODED_RUSTFLAGS = @('-C', 'link-arg=-Wl,-z,max-page-size=16384', '-C', 'link-arg=-Wl,-z,common-page-size=16384') -join [char]31
}
Push-Location $cmProjectDirectory
try {
    $cmBuildLock = Enter-CmBuildLock -ProjectDirectory $cmProjectDirectory
    & (Join-Path $PSScriptRoot 'prepare-android-build.ps1') -ProjectDirectory $cmProjectDirectory
    $env:CMCLOUD_CLI_PREBUILT_NATIVE = '1'
    if ($Release) { $cmPreviousProfile = Enter-CmReleaseProfile -Production:$Production }
    $cmBuildArguments = @('run', 'tauri', '--', 'android', 'build', '--apk', '--target', $Target)
    if (-not $Release) { $cmBuildArguments += '--debug' }
    & npm @cmBuildArguments
    if ($LASTEXITCODE -ne 0) { throw "Android build exited with code $LASTEXITCODE" }
} finally {
    if ($cmBuildLock) { $cmBuildLock.Dispose() }
    if ($cmPreviousProfile) { Exit-CmReleaseProfile -Previous $cmPreviousProfile }
    Pop-Location
    if ($cmUseEncodedRustFlags) { $env:CARGO_ENCODED_RUSTFLAGS = $cmSavedEncodedRustFlags }
    $env:CMCLOUD_CLI_PREBUILT_NATIVE = $cmSavedReuseNative
}
