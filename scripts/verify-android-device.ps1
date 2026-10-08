param(
    [Parameter(Mandatory)][string]$Apk,
    [string]$Serial
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'android-env.ps1')
$cmApkPath = (Resolve-Path -LiteralPath $Apk).Path
if (-not $Serial) {
    $cmDevices = @(adb devices | Select-String '^\S+\s+device$' | ForEach-Object { ($_.Line -split '\s+')[0] })
    if ($cmDevices.Count -ne 1) { throw 'Connect one authorized ADB device, or specify -Serial.' }
    $Serial = $cmDevices[0]
}
$cmAdbArgs = @('-s', $Serial)
$cmUser = (& adb @cmAdbArgs shell am get-current-user).Trim()
if ($LASTEXITCODE -ne 0 -or $cmUser -notmatch '^\d+$') { throw 'Cannot resolve the active Android user.' }
# A named user avoids OEM private-profile permission failures. Push install also
# works when the device rejects ADB streamed installation.
& adb @cmAdbArgs install --user $cmUser --no-streaming -r $cmApkPath
if ($LASTEXITCODE -ne 0) { throw 'Device installation failed; existing app data was not removed.' }
$cmInstalled = @(& adb @cmAdbArgs shell pm path --user $cmUser io.cmcloud.plus) | Where-Object { $_ -match '^package:.*base.apk$' }
if (-not $cmInstalled) { throw 'The application is not installed for the current user.' }
$cmInstalledPath = ($cmInstalled | Select-Object -First 1).Trim().Substring(8)
$cmInstalledHash = (& adb @cmAdbArgs shell sha256sum $cmInstalledPath) -split '\s+' | Select-Object -First 1
if ($LASTEXITCODE -ne 0 -or $cmInstalledHash -ne (Get-FileHash -LiteralPath $cmApkPath).Hash.ToLowerInvariant()) {
    throw 'Device APK differs from the verified local package.'
}
$cmLaunch = & adb @cmAdbArgs shell am start --user $cmUser -W -n io.cmcloud.plus/.MainActivity
if ($LASTEXITCODE -ne 0 -or -not ($cmLaunch -match '^Status: ok$')) { throw 'Device launch failed.' }
Start-Sleep -Seconds 3
$cmAppPidLines = @(& adb @cmAdbArgs shell pidof io.cmcloud.plus)
$cmAppPid = ($cmAppPidLines -join '').Trim()
if (-not $cmAppPid) { throw 'Application is no longer running. Leave it open while verifying; no app data was cleared.' }
$cmReport = [pscustomobject]@{
    package = 'io.cmcloud.plus'
    sdk = (& adb @cmAdbArgs shell getprop ro.build.version.sdk).Trim()
    abi = (& adb @cmAdbArgs shell getprop ro.product.cpu.abilist).Trim()
    activeUser = [int]$cmUser
    pid = $cmAppPid
    installedAPKMatches = $true
    launch = 'ok'
}
$cmReport | ConvertTo-Json
