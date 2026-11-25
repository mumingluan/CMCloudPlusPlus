param(
    [switch]$Release,
    [switch]$Production,
    [switch]$Unsigned,
    [ValidateSet('aarch64', 'armv7', 'x86_64', 'i686')]
    [string]$Target = 'aarch64'
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-env.ps1') -Android
$cmProjectDirectory = Split-Path -Parent $PSScriptRoot

function Sync-CmAndroidProject {
    param([Parameter(Mandatory)][string]$ProjectDirectory)
    $cmTaskPath = Join-Path $ProjectDirectory 'src-tauri/gen/android/buildSrc/src/main/java/io/cmcloud/plus/kotlin/BuildTask.kt'
    if (-not (Test-Path -LiteralPath $cmTaskPath)) {
        throw 'Initialize the Android project with npm run tauri -- android init first.'
    }

    $cmIconSource = Join-Path $ProjectDirectory 'src-tauri/icons/android'
    $cmResourceRoot = Join-Path $ProjectDirectory 'src-tauri/gen/android/app/src/main/res'
    $cmIconPaths = @('values/ic_launcher_background.xml', 'mipmap-anydpi-v26/ic_launcher.xml', 'drawable/ic_launcher_foreground.xml')
    foreach ($cmDensity in @('mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi')) {
        foreach ($cmIconName in @('ic_launcher.png', 'ic_launcher_round.png')) {
            $cmIconPaths += "mipmap-$cmDensity/$cmIconName"
        }
    }
    foreach ($cmIconPath in $cmIconPaths) {
        $cmSource = Join-Path $cmIconSource $cmIconPath
        if (-not (Test-Path -LiteralPath $cmSource -PathType Leaf)) { throw "Missing project launcher resource: $cmIconPath" }
        $cmDestination = Join-Path $cmResourceRoot $cmIconPath
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($cmDestination)) | Out-Null
        if (-not (Test-Path -LiteralPath $cmDestination) -or
            (Get-FileHash -LiteralPath $cmSource).Hash -ne (Get-FileHash -LiteralPath $cmDestination).Hash) {
            Copy-Item -LiteralPath $cmSource -Destination $cmDestination
        }
    }
    $cmAdaptiveIcon = Join-Path $cmResourceRoot 'mipmap-anydpi-v26/ic_launcher.xml'
    $cmRoundAdaptiveIcon = Join-Path $cmResourceRoot 'mipmap-anydpi-v26/ic_launcher_round.xml'
    if (-not (Test-Path -LiteralPath $cmRoundAdaptiveIcon) -or
        (Get-FileHash -LiteralPath $cmAdaptiveIcon).Hash -ne (Get-FileHash -LiteralPath $cmRoundAdaptiveIcon).Hash) {
        Copy-Item -LiteralPath $cmAdaptiveIcon -Destination $cmRoundAdaptiveIcon
    }

    $cmManifestPath = Join-Path $ProjectDirectory 'src-tauri/gen/android/app/src/main/AndroidManifest.xml'
    $cmManifestSource = [IO.File]::ReadAllText($cmManifestPath)
    if (-not $cmManifestSource.Contains('android:roundIcon=')) {
        $cmManifestSource = $cmManifestSource.Replace('android:icon="@mipmap/ic_launcher"',
            'android:icon="@mipmap/ic_launcher"' + "`n        " + 'android:roundIcon="@mipmap/ic_launcher_round"')
        if (-not $cmManifestSource.Contains('android:roundIcon="@mipmap/ic_launcher_round"')) {
            throw 'Unsupported launcher manifest template'
        }
        [IO.File]::WriteAllText($cmManifestPath, $cmManifestSource, [Text.UTF8Encoding]::new($false))
    }

    $cmResolvedResources = (Resolve-Path -LiteralPath $cmResourceRoot).Path
    $cmTemplateIcons = @('drawable/ic_launcher_background.xml', 'drawable-v24/ic_launcher_foreground.xml')
    foreach ($cmDensity in @('mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi')) {
        $cmTemplateIcons += "mipmap-$cmDensity/ic_launcher_foreground.png"
    }
    foreach ($cmTemplateIcon in $cmTemplateIcons) {
        $cmTemplatePath = Join-Path $cmResolvedResources $cmTemplateIcon
        if (Test-Path -LiteralPath $cmTemplatePath) {
            Remove-Item -LiteralPath $cmTemplatePath
        }
    }

    $cmTaskSource = [IO.File]::ReadAllText($cmTaskPath)
    if (-not $cmTaskSource.Contains('CMCLOUD_CLI_PREBUILT_NATIVE')) {
        $cmInsertion = @'
        if (System.getenv("CMCLOUD_CLI_PREBUILT_NATIVE") == "1") {
            val targetName = target ?: throw GradleException("target cannot be null")
            val triple = mapOf("aarch64" to "aarch64-linux-android", "armv7" to "armv7-linux-androideabi", "x86_64" to "x86_64-linux-android", "i686" to "i686-linux-android")[targetName]
                ?: throw GradleException("Unsupported target")
            val abi = mapOf("aarch64" to "arm64-v8a", "armv7" to "armeabi-v7a", "x86_64" to "x86_64", "i686" to "x86")[targetName]!!
            val profile = if (release == true) "release" else "debug"
            val cargoRoot = File(projectDir, rootDirRel ?: throw GradleException("Missing Cargo root")).canonicalFile
            val targetRoot = System.getenv("CARGO_TARGET_DIR")?.let { File(it) } ?: File(cargoRoot, "target")
            val native = File(targetRoot, "$triple/$profile/libcmcloud_lib.so")
            val staged = File(projectDir, "src/main/jniLibs/$abi/libcmcloud_lib.so")
            fun nativeHash(file: File): ByteArray {
                val digest = java.security.MessageDigest.getInstance("SHA-256")
                file.inputStream().use { stream ->
                    val buffer = ByteArray(65536)
                    while (true) {
                        val read = stream.read(buffer)
                        if (read < 0) break
                        digest.update(buffer, 0, read)
                    }
                }
                return digest.digest()
            }
            if (!native.isFile || native.length() == 0L || !staged.isFile || !java.security.MessageDigest.isEqual(nativeHash(staged), nativeHash(native))) {
                throw GradleException("Tauri CLI did not stage the expected native library; refusing stale packaging")
            }
            logger.lifecycle("CMCloud: reuse native library built and staged by Tauri CLI ($targetName/$profile)")
            return
        }
'@
        $cmNeedle = '    fun assemble() {'
        if (-not $cmTaskSource.Contains($cmNeedle)) { throw 'Unsupported Android BuildTask template; no build changes were made.' }
        $cmTaskSource = $cmTaskSource.Replace($cmNeedle, $cmNeedle + "`n" + $cmInsertion)
        [IO.File]::WriteAllText($cmTaskPath, $cmTaskSource, [Text.UTF8Encoding]::new($false))
    }
}

function Invoke-CmPreviewSign {
    param(
        [Parameter(Mandatory)][string]$UnsignedApk,
        [Parameter(Mandatory)][string]$SignedApk
    )
    $cmDebugKeystore = Join-Path $env:USERPROFILE '.android\debug.keystore'
    if (-not (Test-Path -LiteralPath $cmDebugKeystore)) {
        throw 'Android debug.keystore is required for preview signing.'
    }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($SignedApk)) | Out-Null
    $cmBuildTools = Get-ChildItem -LiteralPath (Join-Path $env:ANDROID_HOME 'build-tools') -Directory |
        Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName 'apksigner.bat') } |
        Sort-Object { try { [version]$_.Name } catch { [version]'0.0' } } -Descending |
        Select-Object -First 1 -ExpandProperty FullName
    if (-not $cmBuildTools) { throw 'Android SDK build-tools with apksigner are required.' }

    $cmAlignedApk = Join-Path $env:TEMP ([Guid]::NewGuid().ToString('N') + '-aligned.apk')
    try {
        & (Join-Path $cmBuildTools 'zipalign.exe') -f -P 16 4 $UnsignedApk $cmAlignedApk
        if ($LASTEXITCODE -ne 0) { throw 'Preview APK alignment failed.' }
        & (Join-Path $cmBuildTools 'apksigner.bat') sign --ks $cmDebugKeystore --ks-key-alias androiddebugkey --ks-pass pass:android --key-pass pass:android --v1-signing-enabled true --v2-signing-enabled true --v3-signing-enabled true --v4-signing-enabled false --out $SignedApk $cmAlignedApk
        if ($LASTEXITCODE -ne 0) { throw 'Preview APK signing failed.' }
    } finally {
        if (Test-Path -LiteralPath $cmAlignedApk) { Remove-Item -LiteralPath $cmAlignedApk }
    }
    & (Join-Path $cmBuildTools 'apksigner.bat') verify --verbose --min-sdk-version 23 $SignedApk
    if ($LASTEXITCODE -ne 0) { throw 'Preview APK signature verification failed.' }
    & (Join-Path $cmBuildTools 'zipalign.exe') -c -P 16 4 $SignedApk
    if ($LASTEXITCODE -ne 0) { throw 'Preview APK 16 KB alignment verification failed.' }
    Get-Item -LiteralPath $SignedApk | Select-Object FullName, Length
}

$cmBuildLock = $null
$cmPreviousProfile = $null
$cmSavedEncodedRustFlags = $env:CARGO_ENCODED_RUSTFLAGS
$cmSavedReuseNative = $env:CMCLOUD_CLI_PREBUILT_NATIVE
$cmUseEncodedRustFlags = $Target -in @('aarch64', 'x86_64') -and -not $env:CARGO_ENCODED_RUSTFLAGS -and -not $env:RUSTFLAGS
if ($cmUseEncodedRustFlags) {
    $cmNativeFlags = @('-C', 'link-arg=-Wl,-z,max-page-size=16384', '-C', 'link-arg=-Wl,-z,common-page-size=16384')
    if ($Target -eq 'aarch64') { $cmNativeFlags += @('--cfg', 'aes_armv8') }
    $env:CARGO_ENCODED_RUSTFLAGS = $cmNativeFlags -join [char]31
}

Push-Location $cmProjectDirectory
try {
    $cmBuildLock = Enter-CmBuildLock -ProjectDirectory $cmProjectDirectory
    Sync-CmAndroidProject -ProjectDirectory $cmProjectDirectory
    $env:CMCLOUD_CLI_PREBUILT_NATIVE = '1'
    if ($Release) { $cmPreviousProfile = Enter-CmReleaseProfile -Production:$Production }
    $cmBuildArguments = @('run', 'tauri', '--', 'android', 'build', '--apk', '--target', $Target)
    if (-not $Release) { $cmBuildArguments += '--debug' }
    & npm @cmBuildArguments
    if ($LASTEXITCODE -ne 0) { throw "Android build exited with code $LASTEXITCODE" }

    if ($Release -and -not $Unsigned) {
        $cmVersion = (Get-Content -LiteralPath (Join-Path $cmProjectDirectory 'package.json') -Raw | ConvertFrom-Json).version
        $cmArchLabel = @{ aarch64 = 'arm64'; armv7 = 'armv7'; x86_64 = 'x64'; i686 = 'x86' }[$Target]
        $cmUnsignedApk = Join-Path $cmProjectDirectory 'src-tauri\gen\android\app\build\outputs\apk\universal\release\app-universal-release-unsigned.apk'
        $cmSignedApk = Join-Path $cmProjectDirectory "releases\CMCloudPlusPlus-$cmVersion-android-$cmArchLabel-preview.apk"
        Invoke-CmPreviewSign -UnsignedApk $cmUnsignedApk -SignedApk $cmSignedApk
    }
} finally {
    if ($cmBuildLock) { $cmBuildLock.Dispose() }
    if ($cmPreviousProfile) { Exit-CmReleaseProfile -Previous $cmPreviousProfile }
    Pop-Location
    if ($cmUseEncodedRustFlags) { $env:CARGO_ENCODED_RUSTFLAGS = $cmSavedEncodedRustFlags }
    $env:CMCLOUD_CLI_PREBUILT_NATIVE = $cmSavedReuseNative
}
