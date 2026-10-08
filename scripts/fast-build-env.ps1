# Retain optimized release binaries while avoiding whole-program optimization
# during iterations. Cache variants remain reusable; do not run cargo clean.
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
