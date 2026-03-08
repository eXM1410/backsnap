#!/bin/sh
set -eu

# If snap-pac is present, it already creates pre/post snapshots.
# In that case, skip to avoid duplicate snapshots.
for hook_dir in /etc/pacman.d/hooks /usr/share/libalpm/hooks; do
  if [ -d "$hook_dir" ] && ls "$hook_dir"/*snap-pac* >/dev/null 2>&1; then
    exit 0
  fi
done

# If snapper isn't installed, never block pacman.
if ! command -v snapper >/dev/null 2>&1; then
  exit 0
fi

# Best-effort: never fail the pacman transaction because of a snapshot.
snapper -c root create --type=pre --cleanup-algorithm=number --description="pacman update" >/dev/null 2>&1 || true

exit 0
