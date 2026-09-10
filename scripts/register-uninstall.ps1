# Add the registry entries that make Windows Settings -> Apps
# recognise Bloom as an installed application.
# Per-user install, no admin needed.

$installDir = "$env:LOCALAPPDATA\Programs\Bloom"

# Root key path for installed apps. The reverse-DNS identifier
# (`dev.cnjackson.bloom`) is hardcoded so existing v0.1.x installs
# remain uninstallable. A future release may rename it.
$regPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.cnjackson.bloom"
$appPaths = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\bloom.exe"

$uninstallPs = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$installDir\uninstall.ps1`""

# Remove any prior entry (idempotent)
if (Test-Path $regPath) {
    Remove-Item $regPath -Recurse -Force
}

New-Item -Path $regPath -Force | Out-Null
Set-ItemProperty -Path $regPath -Name "DisplayName"        -Value "Bloom"
Set-ItemProperty -Path $regPath -Name "Publisher"          -Value "ub42"
Set-ItemProperty -Path $regPath -Name "DisplayVersion"     -Value "0.1.0"
Set-ItemProperty -Path $regPath -Name "InstallLocation"    -Value $installDir
Set-ItemProperty -Path $regPath -Name "DisplayIcon"        -Value "$installDir\bloom.exe,0"
Set-ItemProperty -Path $regPath -Name "UninstallString"    -Value $uninstallPs
Set-ItemProperty -Path $regPath -Name "NoModify"           -Value 1 -Type DWord
Set-ItemProperty -Path $regPath -Name "NoRepair"           -Value 1 -Type DWord
Set-ItemProperty -Path $regPath -Name "EstimatedSize"      -Value 12000 -Type DWord  # ~12 MB in KB

# Also write the modern App Paths entry (used by Settings metadata,
# and lets Win32 apps use App Paths to find the exe by name)
New-Item -Path $appPaths -Force | Out-Null
Set-ItemProperty -Path $appPaths -Name "(default)"     -Value "$installDir\bloom.exe"
Set-ItemProperty -Path $appPaths -Name "Path"          -Value $installDir

# Write the uninstall script alongside the exe so the registry
# UninstallString resolves to it. The script uses environment
# variables so it works for whoever is running it.
$uninstallScript = @'
# Bloom per-user uninstall
$dir   = "$env:LOCALAPPDATA\Programs\Bloom"
$start = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Bloom.lnk"
$reg   = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\dev.cnjackson.bloom"
$app   = "HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\bloom.exe"

# Kill the running process if any
Get-Process -Name bloom -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

# Remove files
if (Test-Path $dir)   { Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue }
if (Test-Path $start) { Remove-Item $start -Force -ErrorAction SilentlyContinue }
if (Test-Path $reg)   { Remove-Item $reg -Recurse -Force -ErrorAction SilentlyContinue }
if (Test-Path $app)   { Remove-Item $app -Recurse -Force -ErrorAction SilentlyContinue }

Write-Output "Bloom uninstalled."
'@
Set-Content -Path "$installDir\uninstall.ps1" -Value $uninstallScript -Encoding ASCII

# Verify
if (Test-Path $regPath) {
    $props = Get-ItemProperty $regPath
    Write-Output "REG entry created: $regPath"
    Write-Output "  DisplayName     = $($props.DisplayName)"
    Write-Output "  DisplayVersion  = $($props.DisplayVersion)"
    Write-Output "  InstallLocation = $($props.InstallLocation)"
    Write-Output "  UninstallString = $($props.UninstallString)"
} else {
    Write-Output "ERROR: reg path not created"
    exit 1
}

if (Test-Path "$installDir\uninstall.ps1") {
    Write-Output "uninstall script: $installDir\uninstall.ps1"
} else {
    Write-Output "ERROR: uninstall script not written"
    exit 1
}
