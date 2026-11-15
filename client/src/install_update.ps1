$ErrorActionPreference = 'Stop'
$target = $env:MURR_INSTALL_TARGET
$download = $env:MURR_INSTALL_DOWNLOAD
$stage = [IO.Path]::GetDirectoryName($download)
$backup = [IO.Path]::Combine($stage, 'previous.exe')
$rejected = [IO.Path]::Combine($stage, 'rejected.exe')
$lock = $null
$parentExited = $false
$replaced = $false
$installed = $false
$parentId = [int]$env:MURR_INSTALL_PID

function Start-Game([string]$flag) {
    $start = [Diagnostics.ProcessStartInfo]::new($target, $flag)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WorkingDirectory = [IO.Path]::GetDirectoryName($target)
    $process = [Diagnostics.Process]::Start($start)
    $process.Dispose()
}

function Read-Sha256([string]$path) {
    $stream = [IO.File]::OpenRead($path)
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        [BitConverter]::ToString($hash.ComputeHash($stream)).Replace('-', '')
    } finally {
        $hash.Dispose()
        $stream.Dispose()
    }
}

try {
    $parent = $null
    try { $parent = [Diagnostics.Process]::GetProcessById($parentId) }
    catch [ArgumentException] { }
    if ($parent) {
        try {
            if (-not $parent.WaitForExit(20000)) { return }
        } finally {
            $parent.Dispose()
        }
    }
    $parentExited = $true
    try {
        $lock = [IO.FileStream]::new(
            $target + '.update.lock', [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite,
            [IO.FileShare]::None, 4096, [IO.FileOptions]::DeleteOnClose)
    } catch [IO.IOException] {
        # Another updater is already handling this executable.
        return
    }
    if ((Read-Sha256 $target) -ne $env:MURR_INSTALL_OLD_HASH) {
        return
    }
    if ((Read-Sha256 $download) -ne $env:MURR_INSTALL_NEW_HASH) {
        throw 'The downloaded executable changed'
    }

    [IO.File]::Replace($download, $target, $backup, $false)
    $replaced = $true
    Start-Game '--murr-update-installed'
    $installed = $true
} catch {
    if ([IO.File]::Exists($backup)) {
        try {
            if (-not [IO.File]::Exists($target)) {
                [IO.File]::Move($backup, $target)
            } elseif ($replaced) {
                [IO.File]::Replace($backup, $target, $rejected, $false)
            }
        } catch {
            # Leave the backup in place if Windows cannot restore it.
        }
    }
    if ($parentExited -and [IO.File]::Exists($target)) {
        Start-Game '--murr-update-failed'
    }
} finally {
    if ($lock) { $lock.Dispose() }
    if ([IO.File]::Exists($download)) { [IO.File]::Delete($download) }
    if ([IO.File]::Exists($rejected)) { [IO.File]::Delete($rejected) }
    if ($installed -and [IO.File]::Exists($backup)) { [IO.File]::Delete($backup) }
    if (-not [IO.File]::Exists($backup)) { [IO.Directory]::Delete($stage) }
}
