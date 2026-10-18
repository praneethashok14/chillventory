#!/usr/bin/env bash
# Deploy the Worker and pin the new version to 100% traffic.
#
# Why the explicit pin step: `wrangler deploy` with [assets] sometimes
# promotes an assets-only "shadow" version instead of the one that
# carries our fetch handler. Capturing the Current Version ID from the
# deploy output and running `wrangler versions deploy <id>@100%`
# guarantees the Worker version is the one serving traffic.
#
# Historically this script also did a second pin after a 130s sleep to
# defeat a parallel deploy from a second developer on the same CF
# account. Single-developer now, so the second pin is gone.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"

LOG="$(mktemp -t stockspace-deploy-XXXXXX.log)"
trap 'rm -f "$LOG"' EXIT

npm run prepare-assets

echo "::: wrangler deploy :::"
npx wrangler deploy 2>&1 | tee "$LOG"

VID="$(grep -oE 'Current Version ID: [a-f0-9-]+' "$LOG" | awk '{print $NF}' | tail -1)"
URL="$(grep -oE 'https://[a-z0-9.-]+\.workers\.dev' "$LOG" | head -1)"
if [ -z "$VID" ]; then
  echo "::: ERROR — could not parse Current Version ID :::" >&2
  exit 1
fi

echo
echo "::: pin $VID → 100% traffic :::"
npx wrangler versions deploy "${VID}@100%" --message "auto-pin after npm run deploy" --yes

echo
CODE="$(curl -sS -o /dev/null -w '%{http_code}' "$URL/api/health")"
echo "GET $URL/api/health → HTTP $CODE"
if [ "$CODE" != "200" ]; then
  echo "::: ERROR — /api/health is not 200 after deploy :::" >&2
  exit 1
fi
echo "::: deploy complete :::"
