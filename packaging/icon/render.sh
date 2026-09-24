#!/usr/bin/env bash
# Everything that is drawn from `jack.svg`: the icons a desktop installs, the
# blob the binary carries for X11, and the macOS icon set. Run it after
# changing the drawing, and commit what it writes.
#
# Needs rsvg-convert (librsvg), ImageMagick and python3. On a mac `iconutil`
# builds the .icns as well, over the one written here - same icon, Apple's own
# writer.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

# The sizes an icon theme keeps. The SVG is installed as well, for desktops
# that would rather scale it themselves.
for size in 16 24 32 48 64 128 256 512; do
    mkdir -p "hicolor/${size}x${size}/apps"
    rsvg-convert -w "$size" -h "$size" jack.svg -o "hicolor/${size}x${size}/apps/jack.png"
done

# What the binary carries: 64 square of raw RGBA, which is the one format
# winit takes and the one that needs no decoder to read back.
magick "hicolor/64x64/apps/jack.png" -depth 8 RGBA:../../src/gui/icon.rgba

# macOS's icon set. Its format is a header and a run of tagged PNGs, so it is
# built here rather than only on a mac with `iconutil`.
for size in 1024; do
    rsvg-convert -w "$size" -h "$size" jack.svg -o "jack-${size}.png"
done
python3 - <<'PYTHON'
import struct

# Which tag holds which size, as macOS reads them: the plain sizes, then the
# retina ones, which are the same PNGs at twice the size.
wanted = [
    (b"ic04", "hicolor/16x16/apps/jack.png"),
    (b"ic05", "hicolor/32x32/apps/jack.png"),
    (b"ic07", "hicolor/128x128/apps/jack.png"),
    (b"ic08", "hicolor/256x256/apps/jack.png"),
    (b"ic09", "hicolor/512x512/apps/jack.png"),
    (b"ic11", "hicolor/32x32/apps/jack.png"),
    (b"ic12", "hicolor/64x64/apps/jack.png"),
    (b"ic13", "hicolor/512x512/apps/jack.png"),
    (b"ic14", "jack-1024.png"),
]
blocks = b""
for tag, path in wanted:
    with open(path, "rb") as png:
        data = png.read()
    blocks += tag + struct.pack(">I", len(data) + 8) + data
with open("jack.icns", "wb") as icns:
    icns.write(b"icns" + struct.pack(">I", len(blocks) + 8) + blocks)
PYTHON
rm -f jack-1024.png

if command -v iconutil > /dev/null; then
    rm -rf jack.iconset
    mkdir jack.iconset
    for size in 16 32 128 256 512; do
        cp "hicolor/${size}x${size}/apps/jack.png" "jack.iconset/icon_${size}x${size}.png"
        double=$((size * 2))
        rsvg-convert -w "$double" -h "$double" jack.svg -o "jack.iconset/icon_${size}x${size}@2x.png"
    done
    iconutil -c icns jack.iconset -o jack.icns
    rm -rf jack.iconset
fi
