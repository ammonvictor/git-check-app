#!/usr/bin/env bash
# Build a macOS .app bundle and a .dmg from a git-check-tray binary.
#
# Usage: scripts/macos-bundle.sh <version> <binary> [out-dir]
#
# The bundle declares LSUIElement, so the app has no Dock icon even if the
# runtime accessory-policy call is skipped. The binary is signed ad-hoc: that is
# enough for a local run, and a Developer ID signature belongs with the
# notarization work, not here.
set -euo pipefail

VERSION="${1:?usage: macos-bundle.sh <version> <binary> [out-dir]}"
BINARY="${2:?binary path}"
OUT_DIR="${3:-dist}"

APP_NAME="git-check"
EXECUTABLE="git-check-tray"
APP="$OUT_DIR/$APP_NAME.app"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/$EXECUTABLE"
chmod +x "$APP/Contents/MacOS/$EXECUTABLE"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>${EXECUTABLE}</string>
    <key>CFBundleIdentifier</key>
    <string>org.kwano.git-check</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

# Ad-hoc signing keeps a quarantined copy from being rejected outright.
codesign --force --sign - "$APP"

STAGE="$OUT_DIR/dmg-stage"
DMG="$OUT_DIR/git-check-${VERSION}-macos-universal.dmg"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
echo "built $DMG"
