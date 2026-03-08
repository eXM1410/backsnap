#!/bin/sh
set -e

# Build a pacman package (arclight-bin) from the current working tree.
# This is the ONLY supported way to build & install arclight.
#
# Usage (from anywhere):
#   /home/max/arclight/packaging/arch/build-local.sh
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

RELEASE_BIN="$ROOT_DIR/src-tauri/target/release/arclight"
if [ ! -f "$RELEASE_BIN" ]; then
  echo "ERROR: release binary not found: $RELEASE_BIN" >&2
  exit 1
fi

# Stage artefacts for makepkg
mkdir -p "$PKG_DIR"
cp -f "$RELEASE_BIN" "$PKG_DIR/arclight"
chmod 755 "$PKG_DIR/arclight"
cp -f "$ROOT_DIR/src-tauri/icons/32x32.png"  "$PKG_DIR/32x32.png"
cp -f "$ROOT_DIR/src-tauri/icons/128x128.png" "$PKG_DIR/128x128.png"

# Desktop entry (always regenerate)
cat > "$PKG_DIR/arclight.desktop" <<'EOF'
[Desktop Entry]
Name=Arclight
Comment=System Management, Backup & RGB Control
Exec=/usr/bin/arclight
StartupWMClass=arclight
Icon=arclight
Terminal=false
Type=Application
Categories=System;Utility;
Keywords=backup;btrfs;rgb;nvme;sync;
EOF

# Keep PKGBUILD version in sync
cd "$PKG_DIR"
sed -i "s/^pkgver=.*/pkgver=${PKGVER}/" PKGBUILD

echo "→ Building pacman package…"
makepkg -f

# Clean up stale /usr/local/bin binary if present
if [ -f /usr/local/bin/arclight ]; then
  echo "→ Removing stale /usr/local/bin/arclight…"
  sudo rm -f /usr/local/bin/arclight
fi

PKG_FILE="$(ls -1t "$PKG_DIR"/arclight-bin-*.pkg.tar.zst 2>/dev/null | head -1)"
if [ $DO_INSTALL -eq 1 ] && [ -n "$PKG_FILE" ]; then
  echo "→ Installing via pacman…"
  sudo pacman -U --noconfirm "$PKG_FILE"
  # setcap is also done in post_install hook, but ensure it's set after local builds
  sudo setcap cap_sys_admin+ep /usr/bin/arclight 2>/dev/null || true
  echo "✓ Installed. Binary: $(which arclight) ($(md5sum /usr/bin/arclight | cut -d' ' -f1))"
else
  echo "✓ Done. Install with: sudo pacman -U $PKG_FILE"
fi
