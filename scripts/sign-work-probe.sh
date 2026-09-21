#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
probe="$repo/target/debug/macos-terra-agentic-probe"
keychain="$HOME/Library/Keychains/login.keychain-db"

if [ "$(uname -s)" != Darwin ] || [ ! -f "$probe" ] || [ -L "$probe" ]; then
    echo 'Build the macOS debug probe before signing.' >&2
    exit 1
fi

/usr/bin/codesign --force --sign 'Zephium Dev' --keychain "$keychain" \
    --identifier app.zephium.work-integration.probe --timestamp=none "$probe"
/usr/bin/codesign --verify --strict "$probe"
