$ErrorActionPreference = 'Stop'
$cmProjectRoot = Split-Path -Parent $PSScriptRoot
$cmBuildTemp = Join-Path $cmProjectRoot '.build-tmp'
New-Item -ItemType Directory -Force -Path $cmBuildTemp | Out-Null

# The JVM selector's local wakeup pipe can fail in the inherited Windows TEMP
# directory. Use a short project-local directory for this build and its children;
# leave machine-wide environment settings unchanged.
$env:TEMP = $cmBuildTemp
$env:TMP = $cmBuildTemp
$env:JAVA_TOOL_OPTIONS = (($env:JAVA_TOOL_OPTIONS, ('"-Djava.io.tmpdir=' + $cmBuildTemp + '"')) -join ' ').Trim()

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
        $cmNdkDirectory = Get-ChildItem -LiteralPath (Join-Path $env:ANDROID_HOME 'ndk') -Directory | Sort-Object Name -Descending | Select-Object -First 1 -ExpandProperty FullName
    }
    $env:NDK_HOME = $cmNdkDirectory
}
if (-not $env:NDK_HOME -or -not (Test-Path -LiteralPath $env:NDK_HOME)) { throw 'Android NDK not found. Install an NDK in Android Studio or set NDK_HOME.' }
$env:PATH = (Join-Path $env:JAVA_HOME 'bin') + ';' + (Join-Path $env:ANDROID_HOME 'platform-tools') + ';' + $env:PATH

# Align native ELF segments for both 4 KiB and 16 KiB Android kernels.
foreach ($cmTarget in @('AARCH64', 'X86_64')) {
    $cmRustFlagsName = 'CARGO_TARGET_' + $cmTarget + '_LINUX_ANDROID_RUSTFLAGS'
    $cmExistingRustFlags = [Environment]::GetEnvironmentVariable($cmRustFlagsName, 'Process')
    if ($cmExistingRustFlags -notmatch 'max-page-size') {
        [Environment]::SetEnvironmentVariable($cmRustFlagsName, ($cmExistingRustFlags + ' -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384').Trim(), 'Process')
    }
}

# Cargo plugin sources and the project may be on different Windows drives.
# Kotlin's relative-path incremental cache cannot represent that arrangement.
Set-Item -LiteralPath 'Env:ORG_GRADLE_PROJECT_kotlin.incremental' -Value 'false'
Set-Item -LiteralPath 'Env:ORG_GRADLE_PROJECT_kotlin.compiler.execution.strategy' -Value 'in-process'
