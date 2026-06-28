#!/usr/bin/env bash
# QuantaMind release orchestrator.
#
# Usage:  scripts/release.sh <new-version>     e.g.  scripts/release.sh 0.1.1
#
# Walks: bump → test → build (signed) → manifest → upload-hint → tag.
# Idempotent — safe to re-run after a partial failure.
#
# Prerequisites (one-time):
#   1. Generate the updater signing key pair:
#        mkdir -p ~/.tauri
#        pnpm --dir=frontend exec tauri signer generate -w ~/.tauri/quantamind-updater.key
#      Back up the .key file (1Password). Paste the printed PUBLIC KEY into
#      backend/tauri.conf.json under plugins.updater.pubkey, then commit.
#   2. Set release notes in RELEASE_NOTES.md (top section is read for the
#      release being cut).

set -euo pipefail

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  echo "usage: $0 <new-version>   (e.g. 0.1.1)" >&2
  exit 1
fi
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be MAJOR.MINOR.PATCH (got: $VERSION)" >&2
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

KEY_PATH="${TAURI_KEY_PATH:-$HOME/.tauri/quantamind-updater.key}"
NOTES_FILE="${REPO_ROOT}/RELEASE_NOTES.md"

# ------ 1. Bump version everywhere ------
echo "==> Bumping version to ${VERSION} in 3 manifests"
# backend/Cargo.toml — first `version = "X.Y.Z"` line
sed -i.bak -E "0,/^version = \"[0-9]+\\.[0-9]+\\.[0-9]+\"$/{s//version = \"${VERSION}\"/;}" backend/Cargo.toml
# backend/tauri.conf.json — top-level "version"
sed -i.bak -E "s/\"version\": *\"[0-9]+\\.[0-9]+\\.[0-9]+\"/\"version\": \"${VERSION}\"/" backend/tauri.conf.json
# frontend/package.json
sed -i.bak -E "s/\"version\": *\"[0-9]+\\.[0-9]+\\.[0-9]+\"/\"version\": \"${VERSION}\"/" frontend/package.json
rm -f backend/Cargo.toml.bak backend/tauri.conf.json.bak frontend/package.json.bak

# Refresh Cargo.lock with the new version recorded
( cd backend && cargo update -p quantamind --offline 2>/dev/null || cargo check --offline 2>/dev/null || true )

# ------ 2. Tests ------
echo "==> Running backend tests"
( cd backend && cargo test --lib --color=never )
echo "==> Running frontend tests"
( cd frontend && pnpm test )

# ------ 3. Signed build ------
if [[ ! -f "$KEY_PATH" ]]; then
  echo "error: signing key not found at ${KEY_PATH}." >&2
  echo "       Run 'pnpm --dir=frontend exec tauri signer generate -w ${KEY_PATH}' first." >&2
  exit 2
fi

echo "==> Fetching llama-server sidecar"
bash "${REPO_ROOT}/scripts/fetch-llama-server.sh"

echo "==> Building signed bundle (this can take a few minutes)"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY_PATH")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
( cd frontend && pnpm tauri build --target aarch64-apple-darwin )

# ------ 4. Generate latest.json ------
BUNDLE_DIR="${REPO_ROOT}/backend/target/aarch64-apple-darwin/release/bundle"
TARGZ_PATH="$(ls "${BUNDLE_DIR}/macos/"*.app.tar.gz | head -n1)"
SIG_PATH="${TARGZ_PATH}.sig"
DMG_PATH="$(ls "${BUNDLE_DIR}/dmg/"*.dmg | head -n1)"

if [[ ! -f "$TARGZ_PATH" || ! -f "$SIG_PATH" || ! -f "$DMG_PATH" ]]; then
  echo "error: expected bundle artifacts missing under ${BUNDLE_DIR}" >&2
  ls -la "${BUNDLE_DIR}"/{macos,dmg} 2>/dev/null >&2 || true
  exit 3
fi

SIG_CONTENTS="$(cat "$SIG_PATH")"
PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
NOTES="$(awk -v v="${VERSION}" '
  /^## / { if (found) { exit } if ($2 == v) { found=1; next } }
  found { print }
' "${NOTES_FILE}" 2>/dev/null || echo "Release ${VERSION}.")"
NOTES="${NOTES:-Release ${VERSION}.}"

DIST_DIR="${REPO_ROOT}/dist"
mkdir -p "${DIST_DIR}"
cp "$TARGZ_PATH" "$SIG_PATH" "$DMG_PATH" "${DIST_DIR}/"
TARGZ_NAME="$(basename "$TARGZ_PATH")"

python3 - <<PYEOF > "${DIST_DIR}/latest.json"
import json
m = {
  "version": "${VERSION}",
  "notes": """${NOTES}""",
  "pub_date": "${PUB_DATE}",
  "platforms": {
    "darwin-aarch64": {
      "signature": """${SIG_CONTENTS}""",
      "url": "https://quantamind.co/releases/${TARGZ_NAME}"
    }
  }
}
print(json.dumps(m, indent=2))
PYEOF

echo "==> Artifacts staged in ${DIST_DIR}:"
ls -la "${DIST_DIR}"

# ------ 5. Upload reminder ------
cat <<EOF

==> Upload these four files to https://quantamind.co/releases/ :

      $(basename "$DMG_PATH")            (fresh-install DMG)
      ${TARGZ_NAME}                       (auto-update bundle)
      ${TARGZ_NAME}.sig                   (signature, MUST be at <bundle>.sig)
      latest.json                         (the manifest the app polls)

    Whatever transport you use (scp, rsync, S3 sync, Vercel CLI), all four
    files must end up at exactly:

      https://quantamind.co/releases/<filename>

    The latest.json's "url" field already encodes that path.

EOF

# ------ 6. Notarize (no-op unless Apple Dev ID env vars are set) ------
bash "${REPO_ROOT}/scripts/notarize.sh" "$DMG_PATH" || {
  echo "warning: notarization step failed or skipped — DMG ships unnotarized." >&2
}

# ------ 7. Git tag (optional — only after upload + smoke test) ------
echo "==> Once the upload is verified and you've installed the update on a"
echo "    test machine, tag the release:"
echo "       git tag v${VERSION} && git push origin v${VERSION}"
