param([Parameter(Mandatory)][string]$ProjectDirectory)
$cmTaskPath = Join-Path $ProjectDirectory 'src-tauri/gen/android/buildSrc/src/main/java/io/cmcloud/plus/kotlin/BuildTask.kt'
if (-not (Test-Path -LiteralPath $cmTaskPath)) { throw 'Initialize the Android project with npm run tauri -- android init first.' }

# Android init can recreate template resources. Refresh the launcher resources
# from the committed project icons before every build, including fresh checkouts.
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
}
foreach ($cmIconPath in $cmIconPaths) {
    $cmSource = Join-Path $cmIconSource $cmIconPath
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
# These template vectors are superseded by the project mipmap resources.
$cmResolvedResources = (Resolve-Path -LiteralPath $cmResourceRoot).Path
$cmTemplateIcons = @('drawable/ic_launcher_background.xml', 'drawable-v24/ic_launcher_foreground.xml')
foreach ($cmDensity in @('mdpi', 'hdpi', 'xhdpi', 'xxhdpi', 'xxxhdpi')) {
    $cmTemplateIcons += "mipmap-$cmDensity/ic_launcher_foreground.png"
}
foreach ($cmTemplateIcon in $cmTemplateIcons) {
    $cmTemplatePath = Join-Path $cmResolvedResources $cmTemplateIcon
    if (Test-Path -LiteralPath $cmTemplatePath) {
        $cmResolvedTemplate = (Resolve-Path -LiteralPath $cmTemplatePath).Path
        if (-not $cmResolvedTemplate.StartsWith($cmResolvedResources + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Template icon cleanup is outside Android resources'
        }
        Remove-Item -LiteralPath $cmResolvedTemplate
    }
}

$cmTaskSource = [IO.File]::ReadAllText($cmTaskPath)
if (-not $cmTaskSource.Contains('CMCLOUD_CLI_PREBUILT_NATIVE')) {
    $cmInsertion = @'
        // The project build script invokes Tauri CLI, which has already built and
        // staged the library before starting Gradle. IDE builds keep the normal path.
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
