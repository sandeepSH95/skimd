#!/bin/sh
# Renders assets/icon/AppIcon-macos.svg into assets/AppIcon.icns.
set -e
cd "$(dirname "$0")/.."

SVG=assets/icon/AppIcon-macos.svg
SET=target/AppIcon.iconset
SMALL=target/AppIcon-small.svg
rm -rf "$SET"
mkdir -p "$SET"

# At 16-64px the subtle near-white crossbars vanish; small sizes get a
# variant with solid grey crossbars so the hash stays legible.
sed 's|url(#crossbar-fill)|#9BA1AA|' "$SVG" > "$SMALL"

for size in 16 32 128 256 512; do
    if [ "$size" -le 32 ]; then src="$SMALL"; else src="$SVG"; fi
    rsvg-convert -w "$size" -h "$size" "$src" -o "$SET/icon_${size}x${size}.png"
    double=$((size * 2))
    rsvg-convert -w "$double" -h "$double" "$src" -o "$SET/icon_${size}x${size}@2x.png"
done

iconutil -c icns "$SET" -o assets/AppIcon.icns
echo "Built assets/AppIcon.icns"
