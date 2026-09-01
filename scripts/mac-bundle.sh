#!/bin/sh
# Builds Skimd.app from the release binary and ad-hoc signs it.
set -e
cd "$(dirname "$0")/.."

cargo build --release

APP=target/Skimd.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/skimd "$APP/Contents/MacOS/skimd"
cp assets/Info.plist "$APP/Contents/Info.plist"
if [ -f assets/AppIcon.icns ]; then
    cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
fi

codesign --force -s - "$APP"
echo "Built $APP"
