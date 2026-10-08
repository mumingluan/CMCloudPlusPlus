param(
    [Parameter(Mandatory = $true)]
    [string]$Apk,
    [string]$Output
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'android-env.ps1')

# This existing Android development certificate is only for local preview builds.
# Production releases require the publisher's own stable signing key.
$cmDebugKeystore = Join-Path $env:USERPROFILE '.android\debug.keystore'
if (-not (Test-Path -LiteralPath $cmDebugKeystore)) { throw 'The existing Android debug.keystore is required for a preview signature.' }
$cmUnsignedApk = (Resolve-Path -LiteralPath $Apk).Path
if (-not $Output) {
    $cmPreviewName = [IO.Path]::GetFileNameWithoutExtension($cmUnsignedApk).Replace('-unsigned', '') + '-preview.apk'
    $Output = Join-Path ([IO.Path]::GetDirectoryName($cmUnsignedApk)) $cmPreviewName
}
$cmSignedApk = [IO.Path]::GetFullPath($Output)
if ($cmSignedApk -eq $cmUnsignedApk) { throw 'The preview output must differ from the unsigned input APK.' }
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($cmSignedApk)) | Out-Null
$cmBuildToolsRoot = Join-Path $env:ANDROID_HOME 'build-tools'
$cmBuildTools = Get-ChildItem -LiteralPath $cmBuildToolsRoot -Directory |
    Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'apksigner.bat') } |
    Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $cmBuildTools) { throw 'Android SDK build-tools with apksigner are required.' }
$cmAlignedApk = Join-Path $env:TEMP ([Guid]::NewGuid().ToString('N') + '-aligned.apk')
try {
    & (Join-Path $cmBuildTools 'zipalign.exe') -f -P 16 4 $cmUnsignedApk $cmAlignedApk
    if ($LASTEXITCODE -ne 0) { throw 'Preview APK alignment failed.' }
    & (Join-Path $cmBuildTools 'apksigner.bat') sign --ks $cmDebugKeystore --ks-key-alias androiddebugkey --ks-pass pass:android --key-pass pass:android --v1-signing-enabled true --v2-signing-enabled true --v3-signing-enabled true --v4-signing-enabled false --out $cmSignedApk $cmAlignedApk
    if ($LASTEXITCODE -ne 0) { throw 'Preview APK signing failed.' }
} finally {
    if (Test-Path -LiteralPath $cmAlignedApk) { Remove-Item -LiteralPath $cmAlignedApk }
}
if ($LASTEXITCODE -ne 0) { throw 'Preview APK signing failed.' }
& (Join-Path $cmBuildTools 'apksigner.bat') verify --verbose --min-sdk-version 23 $cmSignedApk
if ($LASTEXITCODE -ne 0) { throw 'Preview APK signature verification failed.' }
& (Join-Path $cmBuildTools 'zipalign.exe') -c -P 16 4 $cmSignedApk
if ($LASTEXITCODE -ne 0) { throw 'Preview APK 16 KB alignment verification failed.' }
Get-Item -LiteralPath $cmSignedApk | Select-Object FullName, Length
