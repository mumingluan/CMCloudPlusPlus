param([switch]$Android)

$ErrorActionPreference = 'Stop'
$cmProjectRoot = Split-Path -Parent $PSScriptRoot

$cmVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (-not (Test-Path -LiteralPath $cmVsWhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++.' }
$cmVsRoot = & $cmVsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if ($LASTEXITCODE -ne 0 -or -not $cmVsRoot) { throw 'Visual C++ x64 build tools were not found.' }
$cmVcRoot = Join-Path $cmVsRoot 'VC'
$cmCompilerVersion = [IO.File]::ReadAllText((Join-Path $cmVcRoot 'Auxiliary/Build/Microsoft.VCToolsVersion.default.txt')).Trim()
$cmCompilerRoot = Join-Path $cmVcRoot "Tools/MSVC/$cmCompilerVersion"
$cmSdkRegistry = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue
$cmSdkRoot = if ($cmSdkRegistry -and $cmSdkRegistry.KitsRoot10) { $cmSdkRegistry.KitsRoot10 } else { Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10' }
$cmSdkVersion = Get-ChildItem -LiteralPath (Join-Path $cmSdkRoot 'Lib') -Directory |
    Where-Object { (Test-Path -LiteralPath (Join-Path $_.FullName 'ucrt/x64/ucrt.lib')) -and (Test-Path -LiteralPath (Join-Path $_.FullName 'um/x64/kernel32.lib')) } |
    Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1 -ExpandProperty Name
if (-not $cmSdkVersion -or -not (Test-Path -LiteralPath "$cmCompilerRoot/lib/x64/msvcrt.lib")) {
    throw 'Install the Windows SDK and Visual C++ x64 runtime libraries.'
}

$cmMsvcBin = "$cmCompilerRoot\bin\Hostx64\x64"
$cmSdkBin = "$cmSdkRoot\bin\$cmSdkVersion\x64"
foreach ($cmEntry in @($cmSdkBin, $cmMsvcBin)) {
    if (($env:PATH -split ';') -notcontains $cmEntry) { $env:PATH = "$cmEntry;$env:PATH" }
}
$env:LIB = "$cmCompilerRoot\lib\x64;$cmSdkRoot\Lib\$cmSdkVersion\ucrt\x64;$cmSdkRoot\Lib\$cmSdkVersion\um\x64"
$env:INCLUDE = "$cmCompilerRoot\include;$cmSdkRoot\Include\$cmSdkVersion\ucrt;$cmSdkRoot\Include\$cmSdkVersion\shared;$cmSdkRoot\Include\$cmSdkVersion\um;$cmSdkRoot\Include\$cmSdkVersion\winrt"
$env:VCToolsInstallDir = "$cmCompilerRoot\"
$env:VCINSTALLDIR = "$cmVcRoot\"
$env:WindowsSdkDir = "$cmSdkRoot\"
$env:WindowsSDKVersion = "$cmSdkVersion\"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "$cmMsvcBin\link.exe"

if ($Android) {
    $cmBuildTemp = if ($env:CMCLOUD_BUILD_TEMP) { $env:CMCLOUD_BUILD_TEMP } else { Join-Path $cmProjectRoot '.build-tmp' }
    New-Item -ItemType Directory -Force -Path $cmBuildTemp | Out-Null
    $env:TEMP = $cmBuildTemp
    $env:TMP = $cmBuildTemp
    $cmTmpFlag = '"-Djava.io.tmpdir=' + $cmBuildTemp + '"'
    if (($env:JAVA_TOOL_OPTIONS -as [string]) -notlike "*$cmBuildTemp*") {
        $env:JAVA_TOOL_OPTIONS = (($env:JAVA_TOOL_OPTIONS, $cmTmpFlag) -join ' ').Trim()
    }

    if (-not $env:JAVA_HOME) {
        $cmBundledJdk = Join-Path $env:ProgramFiles 'Android\Android Studio\jbr'
        if (Test-Path -LiteralPath (Join-Path $cmBundledJdk 'bin\java.exe')) { $env:JAVA_HOME = $cmBundledJdk }
    }
    if (-not $env:ANDROID_HOME) { $env:ANDROID_HOME = Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
    if (-not (Test-Path -LiteralPath $env:ANDROID_HOME)) { throw 'Android SDK not found. Set ANDROID_HOME to your SDK directory.' }
    $env:ANDROID_SDK_ROOT = $env:ANDROID_HOME

    if (-not $env:NDK_HOME) {
        $cmNdkDirectory = Join-Path $env:ANDROID_HOME 'ndk\27.2.12479018'
        if (-not (Test-Path -LiteralPath $cmNdkDirectory)) {
            $cmNdkDirectory = Get-ChildItem -LiteralPath (Join-Path $env:ANDROID_HOME 'ndk') -Directory |
                Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName
        }
        $env:NDK_HOME = $cmNdkDirectory
    }
    if (-not $env:NDK_HOME -or -not (Test-Path -LiteralPath $env:NDK_HOME)) {
        throw 'Android NDK not found. Install an NDK in Android Studio or set NDK_HOME.'
    }

    foreach ($cmAndroidBin in @((Join-Path $env:ANDROID_HOME 'platform-tools'), (Join-Path $env:JAVA_HOME 'bin'))) {
        if ($cmAndroidBin -and (($env:PATH -split ';') -notcontains $cmAndroidBin)) {
            $env:PATH = "$cmAndroidBin;$env:PATH"
        }
    }

    foreach ($cmTarget in @('AARCH64', 'X86_64')) {
        $cmRustFlagsName = "CARGO_TARGET_${cmTarget}_LINUX_ANDROID_RUSTFLAGS"
        $cmExistingFlags = [Environment]::GetEnvironmentVariable($cmRustFlagsName, 'Process')
        if ($cmExistingFlags -notmatch 'max-page-size') {
            $cmExistingFlags = ($cmExistingFlags + ' -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384').Trim()
        }
        if ($cmTarget -eq 'AARCH64' -and $cmExistingFlags -notmatch 'aes_armv8') {
            $cmExistingFlags = ($cmExistingFlags + ' --cfg aes_armv8').Trim()
        }
        [Environment]::SetEnvironmentVariable($cmRustFlagsName, $cmExistingFlags, 'Process')
    }

    Set-Item -LiteralPath 'Env:ORG_GRADLE_PROJECT_kotlin.incremental' -Value 'false'
    Set-Item -LiteralPath 'Env:ORG_GRADLE_PROJECT_kotlin.compiler.execution.strategy' -Value 'in-process'
}

function Enter-CmBuildLock {
    param([Parameter(Mandatory)][string]$ProjectDirectory)
    $cmLockDirectory = Join-Path $ProjectDirectory '.build-tmp'
    $cmLockItem = Get-Item -LiteralPath $cmLockDirectory -Force -ErrorAction SilentlyContinue
    if ($cmLockItem -and $cmLockItem.Target) {
        [IO.Directory]::CreateDirectory($cmLockItem.Target) | Out-Null
    } else {
        [IO.Directory]::CreateDirectory($cmLockDirectory) | Out-Null
    }
    $cmLockPath = Join-Path $cmLockDirectory 'client-build.lock'
    $cmLockWatch = [Diagnostics.Stopwatch]::StartNew()
    $cmLockNoticeShown = $false
    while ($true) {
        try {
            return [IO.File]::Open($cmLockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        } catch [IO.IOException] {
            if ($cmLockWatch.Elapsed.TotalMinutes -ge 60) { throw 'Timed out waiting for the client build lock.' }
            if (-not $cmLockNoticeShown) {
                Write-Host 'Waiting for the other client build to finish...'
                $cmLockNoticeShown = $true
            }
            Start-Sleep -Milliseconds 250
        }
    }
}

function Enter-CmReleaseProfile {
    param([switch]$Production)
    $cmProfilePrevious = @{}
    $cmProfileOverrides = @{
        CARGO_PROFILE_RELEASE_LTO = 'false'
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
        CARGO_PROFILE_RELEASE_INCREMENTAL = 'true'
    }
    foreach ($cmKey in $cmProfileOverrides.Keys) {
        $cmProfilePrevious[$cmKey] = [Environment]::GetEnvironmentVariable($cmKey, 'Process')
        $cmValue = if ($Production) { $null } else { $cmProfileOverrides[$cmKey] }
        [Environment]::SetEnvironmentVariable($cmKey, $cmValue, 'Process')
    }
    return $cmProfilePrevious
}

function Exit-CmReleaseProfile {
    param([hashtable]$Previous)
    foreach ($cmKey in $Previous.Keys) {
        [Environment]::SetEnvironmentVariable($cmKey, $Previous[$cmKey], 'Process')
    }
}
