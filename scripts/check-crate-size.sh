#!/usr/bin/env bash
# check-crate-size.sh — a RATCHET on per-crate source size, so a crate cannot silently
# grow into a machine-killer.
#
# WHY
# ---
# rustc compiles a crate as ONE unit: the frontend holds the whole crate's IR in memory
# at once, so peak build memory tracks the size of the largest crate, not the workspace.
# Measured across the fleet on 2026-07-30:
#
#   accumulate-test-support   169,616 lines   OOMs at 20-30 GB — froze this box 4x in
#                                             one day, killed the OrbStack VM and took
#                                             Concourse + its tunnel down with it
#   temper-test-support       108,078 lines   approaching
#   anvil-test-support         83,812 lines   builds fine today
#
# The cliff is between 83k and 167k. Nothing announced the crossing: accumulate-test-
# support tripled in ONE month (36,912 -> 130,358 lines, May -> June) one track at a
# time, and the first signal anyone got was the machine freezing in July. A check like
# this would have caught it in May, when the fix was a morning's work.
#
# WHY A RATCHET AND NOT A CAP
# ---------------------------
# A flat cap set where it should be (~50k) is RED ON ARRIVAL for every crate already
# over it — anvil-test-support included. A gate that fails the moment it lands teaches
# people to bypass it, and then it protects nothing. So:
#
#   * each crate's CURRENT size is recorded as its baseline and may not GROW past it
#     (plus a small tolerance for ordinary churn);
#   * shrinking a crate lowers its baseline automatically and PERMANENTLY — a split
#     ratchets the bar down and can never be given back;
#   * raising a baseline requires --update-baseline, i.e. an explicit, reviewable,
#     committed decision rather than a silent drift;
#   * a HARD cap still applies, set from the measured danger zone rather than taste.
#
# The point is not to force a split today. It is to make the next crossing LOUD.
#
# USAGE
#   scripts/check-crate-size.sh                    # check (CI mode; non-zero on fail)
#   scripts/check-crate-size.sh --update-baseline  # re-record after a deliberate change
#   scripts/check-crate-size.sh --root ~/Development/temper
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BASELINE_NAME=".crate-size-baseline"
UPDATE=0
# 120k sits between "builds fine" (83k) and "OOMs the machine" (167k). Anything at this
# size is already serialising every build on every worker.
HARD_CAP=120000
# Over this, splitting is worth doing but nothing is broken yet — warn, never fail.
SOFT_CAP=50000
# Ordinary churn shouldn't trip the gate; a 2% drift is noise, 3.5x in a month is not.
TOLERANCE_PCT=2

BASELINE_OVERRIDE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --update-baseline) UPDATE=1; shift;;
    --root) ROOT="$(cd "$2" && pwd)"; shift 2;;
    --hard-cap) HARD_CAP="$2"; shift 2;;
    # Point at a baseline OTHER than the repo's. Exists so the guard can be
    # mutation-tested against a deliberately-lowered baseline without touching the
    # committed one — a ratchet that has never been watched to fail is not a ratchet.
    --baseline) BASELINE_OVERRIDE="$2"; shift 2;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

BASELINE="${BASELINE_OVERRIDE:-$ROOT/$BASELINE_NAME}"

# Every directory holding a Cargo.toml with a [package] section, measured over its own
# src/ + tests/ only — never a nested crate's, or a workspace root would count everything.
measure() {
  while IFS= read -r manifest; do
    local dir crate n
    dir="$(dirname "$manifest")"
    grep -q '^\[package\]' "$manifest" || continue
    crate="$(basename "$dir")"
    n=$(find "$dir/src" "$dir/tests" -name '*.rs' -type f 2>/dev/null \
        | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1+0}')
    [[ -z "$n" || "$n" -eq 0 ]] && continue
    printf '%s %s %s\n' "$crate" "$n" "$dir"
  done < <(find "$ROOT" -name Cargo.toml -not -path '*/target/*' -not -path '*/node_modules/*' \
           -not -path '*/.claude/worktrees/*' 2>/dev/null | sort)
}

CURRENT="$(measure | sort)"

if [[ ! -f "$BASELINE" || "$UPDATE" -eq 1 ]]; then
  printf '# per-crate source line counts — the RATCHET baseline. See scripts/check-crate-size.sh.\n' > "$BASELINE"
  printf '# Shrinking updates automatically; GROWING requires --update-baseline and review.\n' >> "$BASELINE"
  echo "$CURRENT" | awk '{print $1, $2}' >> "$BASELINE"
  echo "crate-size: baseline written to $BASELINE ($(echo "$CURRENT" | wc -l | tr -d ' ') crates)"
  [[ "$UPDATE" -eq 1 ]] && exit 0
fi

fail=0
shrunk=0
while read -r crate n dir; do
  [[ -z "$crate" ]] && continue
  base=$(awk -v c="$crate" '$1==c {print $2}' "$BASELINE" | head -1)
  if [[ -z "$base" ]]; then
    # A new crate starts at its own size — that is the ratchet admitting a new member,
    # not a violation. It is still subject to the hard cap below.
    echo "  NEW  $crate: $n lines (recording as baseline)"
    printf '%s %s\n' "$crate" "$n" >> "$BASELINE"
    base=$n
  fi
  limit=$(( base + base * TOLERANCE_PCT / 100 ))
  if (( n > HARD_CAP )); then
    echo "  FAIL $crate: $n lines exceeds the HARD CAP of $HARD_CAP."
    echo "       A crate this size is a build-memory hazard: rustc holds the whole crate's"
    echo "       IR at once, so this serialises every build and risks OOM. Split it."
    fail=1
  elif (( n > limit )); then
    echo "  FAIL $crate: $n lines exceeds its baseline $base (+${TOLERANCE_PCT}% = $limit)."
    echo "       Growth here is how accumulate-test-support went 36k -> 130k in one month"
    echo "       and started freezing the machine. Split it, or run --update-baseline and"
    echo "       commit the new number as a deliberate decision."
    fail=1
  elif (( n < base )); then
    shrunk=1
    echo "  DOWN $crate: $n lines (was $base) — baseline ratcheted down."
    # Lower it in place: a shrink can never be given back.
    tmp="$(mktemp)"
    awk -v c="$crate" -v v="$n" '$1==c && $0 !~ /^#/ {print c, v; next} {print}' "$BASELINE" > "$tmp"
    mv "$tmp" "$BASELINE"
  fi
  # GENERATED-CODE EXPANSION — the term line count cannot see.
  #
  # Measured 2026-07-31: accumulate-test-support was 168k hand-written lines and needed
  # 34-41 GB of rustc. Line count said "too big", which was right by accident. The
  # actual driver was `tonic::include_proto!`, which PASTES the generated module into
  # every call site: 24,280 generated lines x 112 call sites = ~2.74 MILLION lines, and
  # each copy is a distinct set of types the compiler checks and codegens independently.
  # Deduplicating to one shared copy took peak rustc from 34-41 GB to 3.81 GB WITHOUT
  # touching a single line of the hand-written source.
  #
  # So a crate can pass the line check and still be a build-memory hazard. Count the
  # duplication directly.
  inc=$(grep -rl 'include_proto!' "$dir/src" 2>/dev/null | wc -l | tr -d ' ')
  if [[ -n "$inc" ]] && (( inc > 1 )); then
    echo "  WARN $crate: $inc include_proto! call sites — each pastes the WHOLE generated"
    echo "       module, so the compiler sees roughly $inc copies of it. Declare it once in"
    echo "       a shared module and \`use\` it. This is usually a far bigger memory win"
    echo "       than splitting the crate, and it changes no hand-written logic."
  fi

  if (( n > SOFT_CAP && n <= HARD_CAP )); then
    echo "  WARN $crate: $n lines is over the ${SOFT_CAP}-line soft cap — worth splitting"
    echo "       before it becomes urgent. This is cheap now and expensive once it OOMs."
  fi
done <<< "$CURRENT"

if (( fail )); then
  echo "crate-size: FAILED"
  exit 1
fi
echo "crate-size: OK$( ((shrunk)) && echo ' (baseline ratcheted down — commit it)')"
exit 0
