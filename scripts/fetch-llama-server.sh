#!/usr/bin/env bash
# Fetch the bundled llama-server sidecar for the host platform (Step 3.2).
# Binaries are NOT committed (see .gitignore); run this before `tauri build`
# or local llama.cpp verification. Pinned for reproducibility.
set -euo pipefail

LLAMA_BUILD="${LLAMA_BUILD:-b9386}"
DEST="$(cd "$(dirname "$0")/.." && pwd)/backend/binaries"
TRIPLE="$(rustc -vV | sed -n 's/host: //p')"

case "$TRIPLE" in
  aarch64-apple-darwin)
    ASSET="llama-${LLAMA_BUILD}-bin-macos-arm64.tar.gz"
    EXT="tar.gz"
    LIBS="*.dylib"
    BIN_NAME="llama-server"
    ;;
  x86_64-unknown-linux-gnu)
    ASSET="llama-${LLAMA_BUILD}-bin-ubuntu-x64.zip"
    EXT="zip"
    LIBS="*.so*"
    BIN_NAME="llama-server"
    ;;
  x86_64-pc-windows-msvc)
    ASSET="llama-${LLAMA_BUILD}-bin-win-avx2-x64.zip"
    EXT="zip"
    LIBS="*.dll"
    BIN_NAME="llama-server.exe"
    ;;
  *)
    echo "No pinned llama-server asset for $TRIPLE yet — see docs/cross-platform-builds.md" >&2
    exit 1 ;;
esac

URL="https://github.com/ggml-org/llama.cpp/releases/download/${LLAMA_BUILD}/${ASSET}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading $ASSET ..."
curl -fsSL "$URL" -o "$TMP/llama.archive"

if [ "$EXT" = "tar.gz" ]; then
  tar -xzf "$TMP/llama.archive" -C "$TMP"
else
  unzip -q "$TMP/llama.archive" -d "$TMP"
fi

BIN="$(find "$TMP" -type f -name "$BIN_NAME" | head -n1)"
[ -n "$BIN" ] || { echo "$BIN_NAME not found in archive" >&2; exit 1; }

mkdir -p "$DEST"
# Colocate the binary's sibling libs so it resolves its rpath libs at runtime.
# Bundled as a Tauri resource dir (not externalBin), so a bare name is fine —
# `llama_dir()` resolves this whole directory at runtime.
# shellcheck disable=SC2086
cp "$(dirname "$BIN")"/$LIBS "$DEST"/ 2>/dev/null || true
cp "$BIN" "$DEST/$BIN_NAME"
chmod +x "$DEST/$BIN_NAME"
echo "Installed $DEST/$BIN_NAME (build $LLAMA_BUILD, $TRIPLE)"
