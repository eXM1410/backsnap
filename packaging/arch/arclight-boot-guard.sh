#!/bin/sh
set -eu

# arclight Boot Guard — backup boot entries before kernel update
# Runs as a pacman pre-transaction hook.

GUARD_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/arclight/boot-guard"
ENTRIES_DIR="/boot/loader/entries"

# Check /boot is mounted
if ! mountpoint -q /boot 2>/dev/null; then
    echo "[Boot Guard] WARNUNG: /boot ist nicht gemountet!"
    echo "[Boot Guard] Kernel-Update ohne gemountetes /boot kann zu Mismatch führen."
    # Don't block the transaction, just warn
    exit 0
fi

# Check entries dir exists
if [ ! -d "$ENTRIES_DIR" ]; then
    exit 0
fi

# Create backup directory with timestamp
TS=$(date +%s)
BACKUP_DIR="$GUARD_DIR/backup-$TS"
mkdir -p "$BACKUP_DIR"

# Copy all .conf entries
COUNT=0
for entry in "$ENTRIES_DIR"/*.conf; do
    [ -f "$entry" ] || continue
    cp "$entry" "$BACKUP_DIR/"
    COUNT=$((COUNT + 1))
done

if [ "$COUNT" -eq 0 ]; then
    rmdir "$BACKUP_DIR" 2>/dev/null || true
    exit 0
fi

# Write label
echo "Automatisch (pacman) $(date '+%d.%m.%Y %H:%M')" > "$BACKUP_DIR/label.txt"

# Save current kernel version
uname -r > "$BACKUP_DIR/kernel.txt"

echo "[Boot Guard] $COUNT Boot-Entries gesichert → $BACKUP_DIR"

# Cleanup: keep at most 10 backups
BACKUP_COUNT=$(find "$GUARD_DIR" -maxdepth 1 -type d -name 'backup-*' | wc -l)
if [ "$BACKUP_COUNT" -gt 10 ]; then
    find "$GUARD_DIR" -maxdepth 1 -type d -name 'backup-*' | sort | head -n $((BACKUP_COUNT - 10)) | while read -r old; do
        rm -rf "$old"
    done
fi

exit 0
