#!/usr/bin/env bash
# Build the WhatsApp sidecar for one target, next to this script.
#
# The installer copies the result beside the xConsole executable; `sidecar_path` in
# src-tauri/src/ai/remote/whatsapp.rs also looks here, so a development tree works with
# no copying at all.
#
# CGO stays off on purpose: the SQLite driver is pure Go (modernc.org/sqlite), so every
# target cross-compiles from any host with nothing but the Go toolchain. Turning it on
# would mean a C cross-compiler per platform for a store that holds one session.
set -euo pipefail
cd "$(dirname "$0")"

GOOS="${GOOS:-$(go env GOOS)}"
GOARCH="${GOARCH:-$(go env GOARCH)}"
out="xconsole-whatsapp"
[ "$GOOS" = "windows" ] && out="$out.exe"

echo "building $out for $GOOS/$GOARCH"
CGO_ENABLED=0 GOOS="$GOOS" GOARCH="$GOARCH" go build \
  -trimpath \
  -ldflags="-s -w" \
  -o "$out" .
echo "wrote $(pwd)/$out"
