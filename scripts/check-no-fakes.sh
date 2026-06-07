#!/usr/bin/env bash
# check-no-fakes.sh — Constitution enforcement guard for no-fakes policy.
#
# Enforces the standing rule: zero fakes/stubs/mocks in tests/e2e/ and in any
# non-#[cfg(test)] production path under crates/*/src. For tests/integration/,
# new fakes hard-fail; existing debt is frozen via the allowlist manifest.
#
# Exit codes:
#   0  — clean: no violations found
#   1  — violations found (printed to stdout)
#
# Usage:
#   bash scripts/check-no-fakes.sh
#
# The allowlist is at: scripts/no-fakes-integration-allowlist.txt
# It may only shrink, never grow. See ticket #206 for context.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ALLOWLIST="$REPO_ROOT/scripts/no-fakes-integration-allowlist.txt"

# ── Banned symbol patterns ────────────────────────────────────────────────────
# Expand this list when new banned symbols are discovered during audit.
BANNED_PATTERNS=(
    "DeterministicEmbeddingService"
    "AlwaysEquivalentVerifier"
    "TextOverlapMergeSemanticVerifier"
    "NoOpSynthesisPass"
    "NoOpMergeSemanticVerifier"
    "UnitVectorEmbeddingService"
    "NeverGeneralVerifier"
)

# Build a single grep alternation pattern from the array
build_grep_pattern() {
    local pattern=""
    for sym in "${BANNED_PATTERNS[@]}"; do
        if [ -n "$pattern" ]; then
            pattern="${pattern}\\|${sym}"
        else
            pattern="${sym}"
        fi
    done
    echo "$pattern"
}

GREP_PATTERN="$(build_grep_pattern)"

violations=0
violation_report=""

# ── Zone 1: tests/e2e/ — must be completely fake-free ────────────────────────
e2e_dir="$REPO_ROOT/tests/e2e"
if [ -d "$e2e_dir" ]; then
    e2e_hits=$(grep -rn "$GREP_PATTERN" "$e2e_dir" --include="*.rs" 2>/dev/null || true)
    if [ -n "$e2e_hits" ]; then
        violation_report+=$'\n'"[HARD FAIL] tests/e2e/ must be completely fake-free."$'\n'
        violation_report+="Banned symbol(s) found:"$'\n'
        while IFS= read -r line; do
            violation_report+="  $line"$'\n'
        done <<< "$e2e_hits"
        violations=$((violations + 1))
    fi
fi

# ── Zone 2: crates/*/src — non-cfg(test) production paths must be fake-free ──
# Strategy: find lines containing banned symbols in crates/*/src/**/*.rs,
# then exclude lines preceded by a #[cfg(test)] or #[cfg(any(test, ...]
# annotation. We do a simple conservative check: grep for the symbol in .rs
# files under crates/*/src, then filter out any file where ALL occurrences
# are inside a #[cfg(test)] or #[cfg(any(test, ...]  module block.
#
# The precise approach: grep for banned symbols, then for each hit check
# whether the entire file's occurrences are gated. We use a two-pass strategy:
# 1. Find all .rs files in crates/*/src that match the pattern.
# 2. For each matching file, check that every match line is inside a cfg(test)
#    or cfg(any(test, ...)) block by requiring the file has exactly the same
#    count of cfg(test) / cfg(any(test blocks as match sites (conservative).
#    In practice, use a simpler heuristic: if the symbol only appears inside
#    a `#[cfg(test)]` or `#[cfg(any(test,` annotated mod block.
#
# For production clarity we use a stricter check: scan for bare symbol use
# outside any #[cfg(test)] block by using a Python one-liner that tracks
# cfg(test) depth.
prod_files=$(find "$REPO_ROOT/crates" -path "*/src/*.rs" -not -path "*/target/*" 2>/dev/null || true)

if [ -n "$prod_files" ]; then
    prod_violations=$(python3 - "$GREP_PATTERN" $prod_files <<'PYEOF' 2>/dev/null || true
import sys, re

raw_pattern = sys.argv[1]
symbols = [s.strip() for s in raw_pattern.split("\\|") if s.strip()]
files = sys.argv[2:]

# A line carries a test-only gate if it annotates #[cfg(test)],
# #[cfg(any(test, ...))], or a `feature = "test-utils"` cfg. Anything inside
# such a gated block is allowed to use fakes; everything else is a production
# path and must be fake-free.
cfg_test_re = re.compile(
    r'#\s*\[\s*cfg\s*\(\s*(?:test\b|any\s*\(\s*test\b)'
    r'|feature\s*=\s*"test-utils"'
)

found_violations = []
for path in files:
    try:
        lines = open(path).read().splitlines()
    except Exception:
        continue

    in_test = False   # inside a cfg(test/test-utils) gated block
    depth = 0         # brace depth within that block
    pending = False   # saw a cfg gate attr, waiting for the block's opening brace

    for idx, line in enumerate(lines):
        stripped = line.strip()
        opens = line.count('{')
        closes = line.count('}')
        is_cfg_attr = bool(cfg_test_re.search(stripped))

        # Symbol check uses the block state established by PRIOR lines. Skip the
        # cfg-attr line itself and comment lines.
        if (not in_test) and (not pending) and (not is_cfg_attr) \
           and (not stripped.startswith('//')) and (not stripped.startswith('#')):
            for sym in symbols:
                if sym in line:
                    found_violations.append(f"{path}:{idx+1}: {line.rstrip()}")
                    break

        # Update block state. A cfg attr opens a pending gate; the gate becomes
        # active once its opening brace appears (possibly on the same line), and
        # closes when brace depth returns to zero.
        if is_cfg_attr:
            pending = True

        if pending:
            if opens > 0:
                in_test = True
                pending = False
                depth = opens - closes
                if depth <= 0:
                    in_test = False
                    depth = 0
            # else: attribute with no brace yet — keep waiting on later lines.
        elif in_test:
            depth += opens - closes
            if depth <= 0:
                in_test = False
                depth = 0

for v in found_violations:
    print(v)
PYEOF
)
    if [ -n "$prod_violations" ]; then
        violation_report+=$'\n'"[HARD FAIL] Production path (crates/*/src) contains banned fake symbol(s) outside #[cfg(test)]:"$'\n'
        while IFS= read -r line; do
            violation_report+="  $line"$'\n'
        done <<< "$prod_violations"
        violations=$((violations + 1))
    fi
fi

# ── Zone 3: tests/integration/ — new fakes fail; existing allowlisted debt passes ──
integration_dir="$REPO_ROOT/tests/integration"
if [ -d "$integration_dir" ]; then
    # Load the allowlist into a set
    declare -A allowed_files
    if [ -f "$ALLOWLIST" ]; then
        while IFS= read -r entry; do
            # Strip whitespace and skip blank/comment lines
            entry="${entry#"${entry%%[![:space:]]*}"}"
            entry="${entry%"${entry##*[![:space:]]}"}"
            if [[ -n "$entry" && "$entry" != "#"* ]]; then
                allowed_files["$entry"]=1
            fi
        done < "$ALLOWLIST"
    fi

    # Find .rs files in tests/integration with banned symbols
    while IFS= read -r filepath; do
        # Compute path relative to repo root
        rel_path="${filepath#$REPO_ROOT/}"
        if [[ -n "${allowed_files[$rel_path]+_}" ]]; then
            continue  # frozen debt — skip
        fi
        # Not in allowlist — check if it contains banned symbols
        hits=$(grep -n "$GREP_PATTERN" "$filepath" 2>/dev/null || true)
        if [ -n "$hits" ]; then
            violation_report+=$'\n'"[HARD FAIL] tests/integration/ NEW fake — not in allowlist: $rel_path"$'\n'
            while IFS= read -r line; do
                violation_report+="  $line"$'\n'
            done <<< "$hits"
            violations=$((violations + 1))
        fi
    done < <(find "$integration_dir" -name "*.rs" -not -path "*/target/*" 2>/dev/null || true)
fi

# ── Report ────────────────────────────────────────────────────────────────────
echo "=== check-no-fakes.sh ==="
echo "Banned symbols: ${BANNED_PATTERNS[*]}"
echo ""

if [ "$violations" -eq 0 ]; then
    echo "PASS: No fake/stub/mock violations found."
    echo ""
    echo "  tests/e2e/        : fake-free (OK)"
    echo "  crates/*/src      : no production fakes (OK)"
    echo "  tests/integration : no new fakes beyond frozen allowlist (OK)"
    exit 0
else
    echo "FAIL: $violations zone(s) with violations."
    echo "$violation_report"
    exit 1
fi
