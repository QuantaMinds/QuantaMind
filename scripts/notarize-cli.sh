#!/usr/bin/env bash
# QuantaMind: Apple Developer ID signing + notarization for the HEADLESS CLI
# binary (`qm`). Sibling of notarize.sh (which handles the desktop .app/.dmg).
#
# Usage:  scripts/notarize-cli.sh <path-to-qm-binary>
#
# Env-gated exactly like notarize.sh: if the four required vars aren't all set,
# this is a clean no-op (exit 0) — safe to call from any release flow before an
# Apple Dev account exists. When set, it codesigns the binary with the hardened
# runtime, zips it, submits to Apple's notary service, and waits for the result.
#
# NOTE on stapling: a bare Mach-O binary cannot carry a stapled ticket (only
# .app/.dmg/.pkg can). Notarization is still recorded server-side — Gatekeeper
# resolves it online. This matters from 2026-09-01 (Gatekeeper tightening) and
# is REQUIRED before any Homebrew formula ships this binary.
#
# Required env vars:
#   APPLE_SIGNING_IDENTITY   "Developer ID Application: Name (TEAMID)"
#   APPLE_ID                 Apple ID email
#   APPLE_PASSWORD           App-specific password (appleid.apple.com)
#   APPLE_TEAM_ID            10-char team identifier

set -euo pipefail

BIN_PATH="${1:-}"
if [[ -z "$BIN_PATH" ]]; then
  echo "usage: $0 <path-to-qm-binary>" >&2
  exit 1
fi
if [[ ! -f "$BIN_PATH" ]]; then
  echo "error: no such file: $BIN_PATH" >&2
  exit 1
fi

# The env gate: loudly skipped, never silently (docs/architecture.md#robustness).
missing=()
for v in APPLE_SIGNING_IDENTITY APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
  [[ -z "${!v:-}" ]] && missing+=("$v")
done
if (( ${#missing[@]} > 0 )); then
  echo "notarize-cli: skipped — signing env not set (${missing[*]}). Binary ships unsigned."
  exit 0
fi

echo "==> Codesigning $(basename "$BIN_PATH") with hardened runtime"
codesign --force --options runtime --timestamp \
  --sign "$APPLE_SIGNING_IDENTITY" "$BIN_PATH"
codesign --verify --strict "$BIN_PATH"

echo "==> Submitting to Apple notary service (zip transport)"
ZIP_PATH="$(mktemp -d)/qm-notarize.zip"
ditto -c -k "$BIN_PATH" "$ZIP_PATH"
xcrun notarytool submit "$ZIP_PATH" \
  --apple-id "$APPLE_ID" \
  --password "$APPLE_PASSWORD" \
  --team-id "$APPLE_TEAM_ID" \
  --wait
rm -f "$ZIP_PATH"

echo "==> Notarized. (Bare binaries are not stapled — Gatekeeper verifies online.)"
