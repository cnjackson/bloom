$ErrorActionPreference = "Stop"
# Force-elevate: this script is launched as admin, so we don't re-prompt
# (PowerShell will already be admin if launched with -Verb RunAs).
$src = "C:\Source\AI\projects\bloom\src-tauri\target\release\bloom.exe"
$dst = "C:\Program Files\Bloom\bloom.exe"
# Check current process admin status
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
Write-Output "running as admin: $isAdmin"
if (-not $isAdmin) {
    Write-Output "NOT admin - relaunching self"
    $argsList = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $MyInvocation.MyCommand.Path)
    Start-Process -FilePath "powershell.exe" -ArgumentList $argsList -Verb RunAs -Wait
    exit 0
}
Write-Output "copying $src -> $dst"
try {
    Copy-Item -Path $src -Destination $dst -Force
    Write-Output "copy OK"
} catch {
    Write-Output "copy FAILED: $($_.Exception.Message)"
    exit 1
}
# Verify
$srcHash = (Get-FileHash $src -Algorithm SHA1).Hash
$dstHash = (Get-FileHash $dst -Algorithm SHA1).Hash
Write-Output "src=$srcHash"
Write-Output "dst=$dstHash"
Write-Output "match=$($srcHash -eq $dstHash)"
