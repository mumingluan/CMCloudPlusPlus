$ErrorActionPreference = 'Stop'
# Discover the installed toolchain rather than pinning one machine's VS/SDK versions.
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
if (-not $cmSdkVersion -or -not (Test-Path -LiteralPath "$cmCompilerRoot/lib/x64/msvcrt.lib")) { throw 'Install the Windows SDK and Visual C++ x64 runtime libraries.' }
$env:PATH = "$cmCompilerRoot\bin\Hostx64\x64;$cmSdkRoot\bin\$cmSdkVersion\x64;" + $env:PATH
$env:LIB = "$cmCompilerRoot\lib\x64;$cmSdkRoot\Lib\$cmSdkVersion\ucrt\x64;$cmSdkRoot\Lib\$cmSdkVersion\um\x64"
$env:INCLUDE = "$cmCompilerRoot\include;$cmSdkRoot\Include\$cmSdkVersion\ucrt;$cmSdkRoot\Include\$cmSdkVersion\shared;$cmSdkRoot\Include\$cmSdkVersion\um;$cmSdkRoot\Include\$cmSdkVersion\winrt"
$env:VCToolsInstallDir = "$cmCompilerRoot\"
$env:VCINSTALLDIR = "$cmVcRoot\"
$env:WindowsSdkDir = "$cmSdkRoot\"
$env:WindowsSDKVersion = "$cmSdkVersion\"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "$cmCompilerRoot\bin\Hostx64\x64\link.exe"
