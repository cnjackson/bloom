@echo off
REM Replace installed bloom.exe with the freshly-built one (real icon).
net session >nul 2>&1 || (
  echo [ERROR] Needs admin. Re-run from an elevated cmd.
  exit /b 1
)
copy /Y "C:\Source\AI\projects\bloom\src-tauri\target\release\bloom.exe" "C:\Program Files\Bloom\bloom.exe"
echo [OK] bloom.exe replaced.
exit /b 0
