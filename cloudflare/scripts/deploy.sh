#!/usr/bin/env bash
# Deploy the Worker and pin the new version to 100% traffic, twice, with a
# wait in between — because `wrangler deploy` against a Worker with an
# [assets] binding emits a second deferred "finalise" version ~60-90s later
# that otherwise claims 100% of traffic. The deferred version carries no
# handlers and makes every /api/* return 404.
#
# Flow:
#   1. prepare assets
#   2. wrangler deploy  (uploads Worker + assets, creates version A)
#   3. parse Current Version ID = A
#   4. pin A to 100% traffic         <-- immediate
#   5. verify /api/health = 200
#   6. sleep 120s  (long enough for the mystery version to appear)
#   7. pin A to 100% traffic AGAIN   <-- overrides any mystery deploy
#   8. verify /api/health = 200
#   9. exit non-zero if health failed, so `npm run deploy` fails loudly.
set -euo pipefail

HERE="$(cd "$(dirname "$0")/.." && pwd)"
cd "$HERE"

LOG="$(mktemp -t fridgeflow-deploy-XXXXXX.log)"
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

pin() {
  echo
  echo "::: pin $VID → 100% traffic (round $1) :::"
  npx wrangler versions deploy "${VID}@100%" --message "auto-pin round $1" --yes
}

check_health() {
  local code
  code="$(curl -sS -o /dev/null -w '%{http_code}' "$URL/api/health")"
  echo "GET $URL/api/health → HTTP $code"
  [ "$code" = "200" ]
}

pin 1
check_health || echo "::: first pin verify failed (unusual) :::"

echo
echo "::: waiting 130s for the deferred mystery version to settle :::"
# Progress ticks so you can see it's not hung.
for i in 10 20 30 40 50 60 70 80 90 100 110 120 130; do
  sleep 10
  code="$(curl -sS -o /dev/null -w '%{http_code}' "$URL/api/health" 2>/dev/null || echo '???')"
  echo "  +${i}s  health=$code"
done

pin 2
if check_health; then
  echo
  echo "::: deploy complete, /api/health = 200 :::"
else
  echo
  echo "::: FINAL ERROR — API still unhealthy after double-pin :::" >&2
  exit 1
fi
