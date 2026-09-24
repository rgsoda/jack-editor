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

# macOS's icon set: a header and a run of tagged PNGs, built here rather than
# only on a mac with `iconutil`. The tags are exactly the ones `iconutil`
# itself writes from an iconset, and each holds one size and only that size -
# a picture filed under a tag that means another size is not scaled, it makes
# macOS throw the file out and draw the generic icon instead.
rsvg-convert -w 1024 -h 1024 jack.svg -o jack-1024.png
python3 - <<'PYTHON'
import struct

# Type, and the pixels it holds. `icp4 icp5 icp6` are 16, 32 and 64; the rest
# are points at one or two pixels each - `ic13` is 128 points at two, so 256.
wanted = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
    (b"ic13", 256),
    (b"ic14", 512),
]

def png(size):
    path = f"jack-{size}.png" if size == 1024 else f"hicolor/{size}x{size}/apps/jack.png"
    with open(path, "rb") as file:
        data = file.read()
    # The width and height are the first two words of the IHDR, which is the
    # first chunk: eight bytes of signature, then the chunk header.
    width, height = struct.unpack(">II", data[16:24])
    assert (width, height) == (size, size), f"{path} is {width}x{height}, not {size}"
    return data

blocks = b""
for tag, size in wanted:
    data = png(size)
    blocks += tag + struct.pack(">I", len(data) + 8) + data
with open("jack.icns", "wb") as icns:
    icns.write(b"icns" + struct.pack(">I", len(blocks) + 8) + blocks)
PYTHON
rm -f jack-1024.png

# What the binary hands to macOS at runtime, for a `jack --gui` started from a
# terminal: that has no bundle to take a Dock icon from, so it sets one.
cp "hicolor/256x256/apps/jack.png" ../../src/gui/icon.png

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
