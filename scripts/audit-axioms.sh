#!/usr/bin/env bash
# Assert that every headline theorem depends only on axioms from the permitted set.
#
# Permitted: propext, Quot.sound — Lean's standard classical/quotient axioms.
# A theorem may depend on FEWER than these (close_establishes_post uses only propext);
# that is strictly better and passes. Anything else — a crypto axiom escaping its
# `opaque` wall, or sorryAx from a `sorry` — means the project's central claim is false.
#
# Usage: ./scripts/audit-axioms.sh
# Exit 0 and "AXIOM AUDIT PASSED" on success; non-zero naming the offender otherwise.
set -uo pipefail

cd "$(dirname "$0")/../lean" || exit 1

EXPECTED_COUNT=6
ALLOWED=("propext" "Quot.sound")

OUT=$(lake env lean scripts/AxiomAudit.lean 2>&1)
STATUS=$?

if [ "$STATUS" -ne 0 ]; then
  echo "AXIOM AUDIT FAILED: lean exited $STATUS"
  echo "$OUT"
  exit 1
fi

is_allowed() {
  local ax="$1"
  for a in "${ALLOWED[@]}"; do
    [ "$ax" = "$a" ] && return 0
  done
  return 1
}

seen=0
failed=0

while IFS= read -r line; do
  case "$line" in
    *"does not depend on any axioms"*)
      seen=$((seen + 1))
      continue
      ;;
    *"depends on axioms:"*)
      seen=$((seen + 1))
      thm=${line%%\' depends*}
      thm=${thm#\'}
      axioms=${line#*: [}
      axioms=${axioms%]*}
      IFS=',' read -ra parts <<< "$axioms"
      for raw in "${parts[@]}"; do
        ax=$(echo "$raw" | tr -d '[:space:]')
        [ -z "$ax" ] && continue
        if ! is_allowed "$ax"; then
          echo "AXIOM AUDIT FAILED: $thm depends on '$ax'"
          failed=1
        fi
      done
      ;;
  esac
done <<< "$OUT"

if [ "$failed" -ne 0 ]; then
  echo "--- full output ---"
  echo "$OUT"
  exit 1
fi

# Guard against the audit silently checking nothing: a renamed or deleted theorem
# must fail loudly rather than pass vacuously.
if [ "$seen" -ne "$EXPECTED_COUNT" ]; then
  echo "AXIOM AUDIT FAILED: expected $EXPECTED_COUNT theorems, saw $seen"
  echo "--- full output ---"
  echo "$OUT"
  exit 1
fi

echo "AXIOM AUDIT PASSED: $seen/$EXPECTED_COUNT theorems within [propext, Quot.sound]"
