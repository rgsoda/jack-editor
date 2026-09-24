#!/usr/bin/env bash
# Build `jack.app`, which is how macOS wants a windowed program: a dock icon,
# a name in the menu bar, and somewhere for Launchpad to find it. Homebrew
# installs the binary, not a bundle, so this wraps whichever `jack` is on the
# path - upgrade the formula and the app follows it.
#
#   ./packaging/macos/bundle.sh [where-to-put-it]
#
# With nothing after it, the app goes in /Applications.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
into="${1:-/Applications}"
app="$into/jack.app"

jack="$(command -v jack || true)"
if [ -z "$jack" ]; then
    echo "no jack on the path: brew install rgsoda/tap/jack, or cargo install --path . --features gui" >&2
    exit 1
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$here/../icon/jack.icns" "$app/Contents/Resources/jack.icns"

# The executable is a launcher rather than the editor itself, so that the
# bundle keeps working when the binary underneath it is upgraded, and so that
# the app opens the window rather than a terminal editor with no terminal.
cat > "$app/Contents/MacOS/jack" <<'LAUNCHER'
#!/bin/sh
# A bundle starts with the login environment rather than a shell's, so the
# usual places a jack might live are looked in by hand.
PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
exec jack --gui "$@"
LAUNCHER
chmod +x "$app/Contents/MacOS/jack"

# The version goes in the plist as well as being true: Launch Services keys
# its icon cache on the bundle, and a version that never changes is a cache
# that never notices a new icon.
version="$("$jack" --version | awk '{print $2}')"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>jack</string>
    <key>CFBundleDisplayName</key>
    <string>jack</string>
    <key>CFBundleExecutable</key>
    <string>jack</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.rgsoda.jack</string>
    <key>CFBundleIconFile</key>
    <string>jack</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>
</dict>
</plist>
PLIST

# Launch Services caches an app's icon by path, so a rebuilt app in the same
# place can go on showing the old one - or the generic executable icon it had
# before there was an icon at all. Re-registering and touching the bundle is
# what clears it short of logging out.
touch "$app"
lsregister=/System/Library/Frameworks/CoreServices.framework/Versions/A/Frameworks/LaunchServices.framework/Versions/A/Support/lsregister
[ -x "$lsregister" ] && "$lsregister" -f "$app" || true

echo "$app"
echo "if the Dock or Finder still shows the old icon, it is their cache:"
echo "  killall Dock; killall Finder"
