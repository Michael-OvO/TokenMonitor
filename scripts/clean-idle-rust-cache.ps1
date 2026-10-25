param([switch]$DryRun)

$ErrorActionPreference = 'Stop'
$cacheLock = $null
try {
    $projectRoot = Split-Path -Parent $PSScriptRoot
    $target = [IO.Path]::GetFullPath((Join-Path $projectRoot 'src-tauri\target'))
    $debugCache = Join-Path $target 'debug'
    if (-not (Test-Path -LiteralPath $debugCache)) { return }

    # Never clean a shared/custom Cargo target or a redirected directory.
    $metadataText = & cargo metadata --manifest-path (Join-Path $projectRoot 'src-tauri\Cargo.toml') --no-deps --format-version 1 --offline --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata unavailable' }
    $metadata = $metadataText | ConvertFrom-Json
    if ([IO.Path]::GetFullPath($metadata.target_directory) -ne $target) { throw 'Custom target directory; automatic cleanup skipped' }
    $ancestor = $debugCache
    while ($ancestor) {
        if ((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Redirected cache path' }
        $ancestor = Split-Path -Parent $ancestor
    }
    if (@(Get-Process | Where-Object { $_.ProcessName -match '^(cargo|rustc|rust-analyzer|token-monitor|TokenMonitor)(-|$)' }).Count) {
        Write-Output '[rust-cache] Build/debug process active; keeping cache.'
        return
    }

    # Cargo uses debug/.cargo-lock. An exclusive Windows handle prevents a build
    # from acquiring it during inspection/deletion; never delete the lock itself.
    $lockPath = Join-Path $debugCache '.cargo-lock'
    $cacheLock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $entries = @(Get-ChildItem -LiteralPath $debugCache -Recurse -Force)
    if (@($entries | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) { throw 'Cache contains redirected entries' }
    $entries = @($entries | Where-Object { $_.FullName -ne $lockPath })
    if (-not $entries.Count) { return }
    $cutoff = [DateTime]::UtcNow.AddDays(-7)
    if (@($entries | Where-Object { $_.LastWriteTimeUtc -ge $cutoff }).Count) {
        Write-Output '[rust-cache] Cache updated within 7 days; keeping it for fast rebuilds.'
        return
    }
    # The resolved absolute deletion targets must be direct children of debug.
    $children = @(Get-ChildItem -LiteralPath $debugCache -Force | Where-Object { $_.FullName -ne $lockPath })
    foreach ($child in $children) {
        $resolved = (Resolve-Path -LiteralPath $child.FullName).ProviderPath
        if ((Split-Path -Parent $resolved) -ne $debugCache) { throw 'Cleanup path escaped debug directory' }
    }
    if ($DryRun) {
        Write-Output "[rust-cache] Would remove $($children.Count) entries from the development cache (idle for 7 days)."
        return
    }
    foreach ($child in $children) { Remove-Item -LiteralPath $child.FullName -Recurse -Force }
    Write-Output '[rust-cache] Removed development cache idle for at least 7 days. The next build will regenerate it.'
} catch {
    Write-Output "[rust-cache] Cleanup skipped: $($_.Exception.Message)"
} finally {
    if ($cacheLock) { $cacheLock.Dispose() }
}
