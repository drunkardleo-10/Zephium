#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
probe="$repo/target/debug/macos-terra-agentic-probe"
keychain="$HOME/Library/Keychains/login.keychain-db"

if [ "$(uname -s)" != Darwin ] || [ ! -f "$probe" ] || [ -L "$probe" ]; then
    echo 'Build the macOS debug probe before signing.' >&2
    exit 1
fi

identity=${APPLE_SIGNING_IDENTITY:-}
if [ -z "$identity" ]; then
    identities=$(security find-identity -v -p codesigning 2>/dev/null || true)
    identity=$(printf '%s\n' "$identities" | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -n 1)
    if [ -z "$identity" ]; then
        identity=$(printf '%s\n' "$identities" | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' | head -n 1)
    fi
fi
if [ -z "$identity" ]; then
    echo 'No code signing identity found. Set APPLE_SIGNING_IDENTITY or install a "Developer ID Application" or "Apple Development" certificate.' >&2
    exit 1
fi

/usr/bin/codesign --force --sign "$identity" --keychain "$keychain" \
    --identifier app.zephium.work-integration.probe --timestamp=none "$probe"
/usr/bin/codesign --verify --strict "$probe"
