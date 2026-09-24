#!/usr/bin/env bash
# The desktop's half of the window: the entry that puts jack in the launcher
# and the icons it draws it with. The binary is installed by cargo or brew;
# this is what tells the desktop about it.
#
#   ./packaging/linux/install.sh
#
# Into ~/.local/share by default, or wherever $XDG_DATA_HOME says. Pass a
# directory to put it somewhere else - /usr/local/share for everyone.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
into="${1:-${XDG_DATA_HOME:-$HOME/.local/share}}"

install -Dm644 "$here/../jack.desktop" "$into/applications/jack.desktop"
install -Dm644 "$here/../icon/jack.svg" "$into/icons/hicolor/scalable/apps/jack.svg"
for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$here/../icon/hicolor/${size}x${size}/apps/jack.png" \
        "$into/icons/hicolor/${size}x${size}/apps/jack.png"
done

# `jack-gui` is the same binary under a name that opens a window - what the
# desktop entry could do with a flag, but a Dock item or a file association
# cannot. Beside whichever jack is on the path, since that is the one the
# launcher will find.
jack="$(command -v jack || true)"
if [ -n "$jack" ]; then
    jack="$(readlink -f "$jack")"
    if ln -sfn "$jack" "$(dirname "$jack")/jack-gui" 2> /dev/null; then
        echo "jack-gui is beside $jack"
    else
        echo "could not write beside $jack - for a windowed name, run:"
        echo "  sudo ln -sfn $jack $(dirname "$jack")/jack-gui"
    fi
fi

# Both caches are hints rather than requirements: a desktop that has neither
# tool reads the directories itself.
command -v gtk-update-icon-cache > /dev/null && gtk-update-icon-cache -q -f -t "$into/icons/hicolor" || true
command -v update-desktop-database > /dev/null && update-desktop-database "$into/applications" || true

echo "jack.desktop and its icons are in $into"
