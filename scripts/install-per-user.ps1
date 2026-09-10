# Create a per-user Start-menu shortcut for Bloom. Works for any user
# without modification - paths are read from environment variables.
$ws = New-Object -ComObject WScript.Shell
$shortcut = $ws.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk")
$shortcut.TargetPath = "$env:LOCALAPPDATA\Programs\Bloom\bloom.exe"
$shortcut.WorkingDirectory = "$env:LOCALAPPDATA\Programs\Bloom"
$shortcut.IconLocation = "$env:LOCALAPPDATA\Programs\Bloom\bloom.exe,0"
$shortcut.Description = "Bloom - system-wide text replacement"
$shortcut.Save()
Write-Output "shortcut created: $env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk"

# Verify
if (Test-Path "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk") {
    $s = Get-Item "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk"
    Write-Output "verified exists: $($s.Length) bytes"
} else {
    Write-Output "FAILED: shortcut not created"
    exit 1
}
