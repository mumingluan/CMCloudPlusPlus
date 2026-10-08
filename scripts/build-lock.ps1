function Enter-CmBuildLock {
    param([Parameter(Mandatory = $true)][string]$ProjectDirectory)
    # Hold the same OS file lock through Vite and native compilation. Vite clears
    # shared dist before writing it; Rust must not embed that temporary empty tree.
    $cmLockDirectory = Join-Path $ProjectDirectory '.build-tmp'
    [IO.Directory]::CreateDirectory($cmLockDirectory) | Out-Null
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
