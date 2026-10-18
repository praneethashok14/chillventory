#!/usr/bin/env bash
# Deploy the Worker and immediately pin the newly-uploaded version to 100%
# traffic. Workaround for a wrangler v4 behaviour where `wrangler deploy`
# against a Worker with an [assets] binding sometimes promotes a shadow
# assets-only version instead of our Worker, leaving every /api/* route 404.
# See docs/history: the mystery versions have no handlers and no bindings.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"

LOG="$(mktemp -t fridgeflow-deploy-XXXXXX.log)"
trap 'rm -f "$LOG"' EXIT

npm run prepare-assets

echo "::: wrangler deploy :::"
npx wrangler deploy 2>&1 | tee "$LOG"

VID="$(grep -oE 'Current Version ID: [a-f0-9-]+' "$LOG" | awk '{print $NF}' | tail -1)"
if [ -z "$VID" ]; then
  echo "::: ERROR — could not parse Current Version ID from deploy log :::" >&2
  exit 1
fi

echo
echo "::: pinning version $VID to 100% traffic :::"
npx wrangler versions deploy "${VID}@100%" --message "auto-pin after npm run deploy" --yes

echo
echo "::: deploy complete; verifying :::"
URL="$(grep -oE 'https://[a-z0-9.-]+\.workers\.dev' "$LOG" | head -1)"
if [ -n "$URL" ]; then
  CODE="$(curl -sS -o /dev/null -w '%{http_code}' "$URL/api/health")"
  echo "GET $URL/api/health → HTTP $CODE"
  if [ "$CODE" != "200" ]; then
    echo "::: WARNING — /api/health did not return 200 :::" >&2
    exit 1
  fi
fi
