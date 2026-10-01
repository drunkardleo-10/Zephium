#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
probe="$repo/target/debug/macos-terra-agentic-probe"
bundle="$repo/target/debug/bundle/macos/Zephium Work Probe.app"
keychain="$HOME/Library/Keychains/login.keychain-db"

if [ "$(uname -s)" != Darwin ] || [ ! -f "$probe" ] || [ -L "$probe" ]; then
    echo 'Build the macOS debug Work probe before bundling.' >&2
    exit 1
fi

mkdir -p "$bundle/Contents/MacOS"
cp "$probe" "$bundle/Contents/MacOS/macos-terra-agentic-probe"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.zephium.work-integration.work-probe</string>
<key>CFBundleName</key><string>Zephium Work Probe</string>
<key>CFBundleExecutable</key><string>macos-terra-agentic-probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
/usr/bin/codesign --force --sign 'Developer ID Application: Edgar Injighulyan (4FLB46KK27)' --keychain "$keychain" \
    --timestamp=none "$bundle"
/usr/bin/codesign --verify --deep --strict "$bundle"
