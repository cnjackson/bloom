# Create a per-user Start-menu shortcut for Bloom
$ws = New-Object -ComObject WScript.Shell
$shortcut = $ws.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk")
$shortcut.TargetPath = "C:\Users\cnjac\AppData\Local\Programs\Bloom\bloom.exe"
$shortcut.WorkingDirectory = "C:\Users\cnjac\AppData\Local\Programs\Bloom"
$shortcut.IconLocation = "C:\Users\cnjac\AppData\Local\Programs\Bloom\bloom.exe,0"
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
