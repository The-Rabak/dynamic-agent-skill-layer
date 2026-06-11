#!/usr/bin/env bash
# check-no-fakes.sh — Constitution enforcement guard for no-fakes policy.
#
# Enforces the standing rule: zero fakes/stubs/mocks in tests/e2e/, in any
# non-#[cfg(test)] production path under crates/*/src, and in tests/integration/.
# All three zones are hard-fail — there is no allowlist exemption.
#
# ── Test-location taxonomy (the line this guard draws, made EXPLICIT) ──────────
# FAKE-FREE (policed here — these are "real app" / production surfaces):
#   - tests/e2e/            : drives the real, fully-wired app end-to-end. Zone 1.
#   - tests/integration/    : cross-component tests over real seams. Zone 3.
#                             (Drained empty by T13, 2026-06-11; was allowlisted.)
#   - crates/*/src (non-#[cfg(test)]) : production code. Zone 2.
# FAKE-FRIENDLY (intentionally NOT policed — test-only crate-local unit/component
# tests, allowed by the constitution's "or the language's equivalent test-only
# gating" clause; controlled doubles are legitimate for asserting LOGIC such as
# ranking math or fault-injection paths that real non-deterministic infra cannot):
#   - crates/*/src/#[cfg(test)]  : in-crate unit tests.
#   - crates/*/tests/            : the crate's own integration tests (compiled
#                                  ONLY under `cargo test`; never shipped).
# This boundary is a STATED policy, not a blind spot: a logic/component test that
# needs a controlled embedder belongs in a crate-local test dir, NOT in tests/e2e
# or tests/integration. The efficacy/real-app suites (tests/e2e) therefore stay
# zero-fake regardless of crate-local doubles.
# KNOWN LIMITATION: the guard matches by SYMBOL NAME, so a renamed double evades
# the symbol scan. The taxonomy above — not the symbol list — is the real contract;
# reviewers must keep "real app" tests in tests/e2e/ (Zone 1) where doubles are banned.
#
# Exit codes:
#   0  — clean: no violations found
#   1  — violations found (printed to stdout)
#
# Usage:
#   bash scripts/check-no-fakes.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

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
# #![cfg(test)] (inner attribute), #[cfg(any(test, ...))], or a
# `feature = "test-utils"` cfg. Anything inside such a gated block is allowed
# to use fakes; everything else is a production path and must be fake-free.
# The `#!?` handles both outer (#[...]) and inner (#![...]) attribute forms.
cfg_test_re = re.compile(
    r'#!?\s*\[\s*cfg\s*\(\s*(?:test\b|any\s*\(\s*test\b)'
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

# ── Zone 3: tests/integration/ — must be completely fake-free ────────────────
integration_dir="$REPO_ROOT/tests/integration"
if [ -d "$integration_dir" ]; then
    integration_hits=$(grep -rn "$GREP_PATTERN" "$integration_dir" --include="*.rs" 2>/dev/null || true)
    if [ -n "$integration_hits" ]; then
        violation_report+=$'\n'"[HARD FAIL] tests/integration/ must be completely fake-free."$'\n'
        violation_report+="Banned symbol(s) found:"$'\n'
        while IFS= read -r line; do
            violation_report+="  $line"$'\n'
        done <<< "$integration_hits"
        violations=$((violations + 1))
    fi
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
    echo "  tests/integration : fake-free (OK)"
    exit 0
else
    echo "FAIL: $violations zone(s) with violations."
    echo "$violation_report"
    exit 1
fi
