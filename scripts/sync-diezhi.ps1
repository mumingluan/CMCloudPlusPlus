param(
    [switch]$Apply,
    [switch]$SkipBuild,
    [int]$Workers = 4,
    [string]$WorkDirectory = 'R:/Temp/cmcloud-mirror-diezhi'
)
$ErrorActionPreference = 'Stop'
$cmProject = Split-Path -Parent $PSScriptRoot
$env:TEMP = 'R:/Temp'
$env:TMP = 'R:/Temp'
$env:PYTHONIOENCODING = 'utf-8'
$env:PYTHONPYCACHEPREFIX = 'R:/Temp/cmcloud-mirror/pycache'
New-Item -ItemType Directory -Force -Path 'R:/Temp/cmcloud-mirror', $WorkDirectory | Out-Null
Push-Location $cmProject
try {
    if (-not $SkipBuild) {
        . "$PSScriptRoot/windows-env.ps1"
        . "$PSScriptRoot/fast-build-env.ps1"
        $cmProfile = Enter-CmReleaseProfile
        try {
            cargo build --manifest-path src-tauri/Cargo.toml --example cmcloud_sync_bridge --release --features custom-protocol *> R:/Temp/cmcloud-mirror/build.log
            if ($LASTEXITCODE -ne 0) { throw '同步接口构建失败，请查看 R:/Temp/cmcloud-mirror/build.log' }
        } finally { Exit-CmReleaseProfile -Previous $cmProfile }
    }
    $cmArguments = @('-u', "$PSScriptRoot/cmcloud_mirror.py", '--local', 'D:/Diezhi', '--remote', 'Diezhi', '--work-dir', $WorkDirectory, '--workers', "$Workers")
    if ($Apply) { $cmArguments += '--apply' }
    python @cmArguments
    if ($LASTEXITCODE -ne 0) { throw '同步未完成，请查看工作目录中的日志和差异报告' }
} finally { Pop-Location }
