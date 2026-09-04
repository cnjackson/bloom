#!/bin/bash
# Bloom per-user uninstall
set -e
BLOOM_DIR="$LOCALAPPDATA/Programs/Bloom"
SHORTCUT="$APPDATA/Microsoft/Windows/Start Menu/Programs/Bloom.lnk"
# Tray autostart registry (if set)
REG_PATH="HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
REG_NAME="com.tauri.dev.cnjackson.bloom"

echo "=== killing any running bloom.exe ==="
taskkill //F //IM bloom.exe //T 2>/dev/null || true

echo "=== removing installed dir ==="
rm -rf "$BLOOM_DIR" 2>/dev/null && echo "  removed: $BLOOM_DIR" || echo "  $BLOOM_DIR not present"

echo "=== removing start menu shortcut ==="
rm -f "$SHORTCUT" 2>/dev/null && echo "  removed: $SHORTCUT" || echo "  $SHORTCUT not present"

echo "=== removing tray autostart registry entry ==="
if reg query "$REG_PATH" //v "$REG_NAME" >/dev/null 2>&1; then
    reg delete "$REG_PATH" //v "$REG_NAME" //f >/dev/null 2>&1
    echo "  removed: $REG_NAME"
else
    echo "  $REG_NAME not present"
fi

echo "=== final state ==="
ls "$BLOOM_DIR" 2>/dev/null || echo "  install dir: gone"
ls "$SHORTCUT" 2>/dev/null || echo "  start shortcut: gone"
tasklist | grep -i bloom | head || echo "  no running bloom"
