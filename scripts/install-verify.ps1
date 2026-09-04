Start-Process -FilePath "C:\Source\AI\projects\bloom\scripts\install-upgrade.cmd" -Verb RunAs -Wait -PassThru
$result = $LASTEXITCODE
# Also verify the file copy happened
$src = "C:\Source\AI\projects\bloom\src-tauri\target\release\bloom.exe"
$dst = "C:\Program Files\Bloom\bloom.exe"
$srcHash = (Get-FileHash $src -Algorithm SHA1).Hash
$dstHash = if (Test-Path $dst) { (Get-FileHash $dst -Algorithm SHA1).Hash } else { "missing" }
Write-Output "src=$srcHash"
Write-Output "dst=$dstHash"
Write-Output "match=$($srcHash -eq $dstHash)"
