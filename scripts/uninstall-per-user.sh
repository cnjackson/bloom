#!/bin/bash
# Bloom per-user uninstall
set -e
BLOOM_DIR="$LOCALAPPDATA/Programs/Bloom"
SHORTCUT="$APPDATA/Microsoft/Windows/Start Menu/Programs/Bloom.lnk"
# Autostart registry keys Bloom may have written. Includes the
# current identifier plus any future renames.
REG_PATH="HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
LEGACY_REG_NAMES=(
    "com.tauri.dev.cnjackson.bloom"   # v0.1.0 / v0.1.1
    "com.bloom.app"                   # future releases
)
# Modern Settings -> Apps and App Paths entries.
LEGACY_REG_KEYS=(
    "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\dev.cnjackson.bloom"
    "HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\bloom.exe"
)

echo "=== killing any running bloom.exe ==="
taskkill //F //IM bloom.exe //T 2>/dev/null || true

echo "=== removing installed dir ==="
if [ -d "$BLOOM_DIR" ]; then
    rm -rf "$BLOOM_DIR" && echo "  removed: $BLOOM_DIR"
else
    echo "  $BLOOM_DIR not present"
fi

echo "=== removing start menu shortcut ==="
if [ -f "$SHORTCUT" ]; then
    rm -f "$SHORTCUT" && echo "  removed: $SHORTCUT"
else
    echo "  $SHORTCUT not present"
fi

echo "=== removing tray autostart registry entries ==="
for reg_name in "${LEGACY_REG_NAMES[@]}"; do
    if reg query "$REG_PATH" //v "$reg_name" >/dev/null 2>&1; then
        reg delete "$REG_PATH" //v "$reg_name" //f >/dev/null 2>&1 \
            && echo "  removed: $reg_name" \
            || echo "  failed to remove: $reg_name"
    else
        echo "  $reg_name not present"
    fi
done

echo "=== removing Settings -> Apps / App Paths entries ==="
for reg_key in "${LEGACY_REG_KEYS[@]}"; do
    if reg query "$reg_key" >/dev/null 2>&1; then
        reg delete "$reg_key" //f >/dev/null 2>&1 \
            && echo "  removed: $reg_key" \
            || echo "  failed to remove: $reg_key"
    else
        echo "  $reg_key not present"
    fi
done

echo "=== final state ==="
if [ -d "$BLOOM_DIR" ]; then
    echo "  install dir: still present"
else
    echo "  install dir: gone"
fi
if [ -f "$SHORTCUT" ]; then
    echo "  start shortcut: still present"
else
    echo "  start shortcut: gone"
fi
if tasklist | grep -i bloom >/dev/null 2>&1; then
    echo "  running bloom: yes"
else
    echo "  no running bloom"
fi
