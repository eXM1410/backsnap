#!/bin/sh
set -e

# Build a pacman package (backsnap-bin) from the current working tree.
# This is the ONLY supported way to build & install backsnap.
#
# Usage (from anywhere):
#   /home/max/backsnap/packaging/arch/build-local.sh
#   # or:
#   cd packaging/arch && ./build-local.sh
#
# With --install flag it will also install via pacman:
#   ./build-local.sh --install

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
PKG_DIR="$ROOT_DIR/packaging/arch"
cd "$ROOT_DIR"

DO_INSTALL=0
for arg in "$@"; do
  case "$arg" in
    --install) DO_INSTALL=1 ;;
  esac
done

# Extract version from src-tauri/tauri.conf.json (fallback to 0.1.0)
PKGVER="$(python -c "import json, pathlib; p=pathlib.Path('src-tauri/tauri.conf.json');
try:
  print(json.loads(p.read_text()).get('version','0.1.0'))
except Exception:
  print('0.1.0')
")"

echo "→ Building release binary…"
npm run tauri build -- --no-bundle 2>&1

RELEASE_BIN="$ROOT_DIR/src-tauri/target/release/backsnap"
if [ ! -f "$RELEASE_BIN" ]; then
  echo "ERROR: release binary not found: $RELEASE_BIN" >&2
  exit 1
fi

# Stage artefacts for makepkg
mkdir -p "$PKG_DIR"
cp -f "$RELEASE_BIN" "$PKG_DIR/backsnap"
chmod 755 "$PKG_DIR/backsnap"
cp -f "$ROOT_DIR/src-tauri/icons/32x32.png"  "$PKG_DIR/32x32.png"
cp -f "$ROOT_DIR/src-tauri/icons/128x128.png" "$PKG_DIR/128x128.png"

# Desktop entry (always regenerate)
cat > "$PKG_DIR/backsnap.desktop" <<'EOF'
[Desktop Entry]
Name=backsnap
Comment=System Backup & Recovery Manager
Exec=/usr/bin/backsnap
StartupWMClass=backsnap
Icon=backsnap
Terminal=false
Type=Application
Categories=System;Utility;
Keywords=backup;btrfs;snapper;sync;
EOF

# Keep PKGBUILD version in sync
cd "$PKG_DIR"
sed -i "s/^pkgver=.*/pkgver=${PKGVER}/" PKGBUILD

echo "→ Building pacman package…"
makepkg -f

# Clean up stale /usr/local/bin binary if present
if [ -f /usr/local/bin/backsnap ]; then
  echo "→ Removing stale /usr/local/bin/backsnap…"
  sudo rm -f /usr/local/bin/backsnap
fi

PKG_FILE="$(ls -1t "$PKG_DIR"/backsnap-bin-*.pkg.tar.zst 2>/dev/null | head -1)"
if [ $DO_INSTALL -eq 1 ] && [ -n "$PKG_FILE" ]; then
  echo "→ Installing via pacman…"
  sudo pacman -U --noconfirm "$PKG_FILE"
  echo "✓ Installed. Binary: $(which backsnap) ($(md5sum /usr/bin/backsnap | cut -d' ' -f1))"
else
  echo "✓ Done. Install with: sudo pacman -U $PKG_FILE"
fi
