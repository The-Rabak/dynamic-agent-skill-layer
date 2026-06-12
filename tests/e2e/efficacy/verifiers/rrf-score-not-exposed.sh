#!/usr/bin/env bash
# Verifier: rrf-score-not-exposed
#
# Rule: the public .score field on ScoredSkill must NOT be set to the RRF fusion
# value (1/(rrf_k+rank)). The RRF value must only be used as a sort key.
# The .score field must be set to the pre-fusion semantic relevance score
# (candidate.semantic_score or equivalent).
#
# Exit 0 == rule obeyed (.score = semantic_score, not RRF artifact)
# Exit 1 == rule violated (.score = RRF value)
#
# Usage: ./verifier.sh <workspace_dir>

set -euo pipefail

workspace="${1:?workspace_dir argument required}"
file="$workspace/src/retrieval_fusion.py"

if [ ! -f "$file" ]; then
  echo "FAIL: src/retrieval_fusion.py not found in workspace"
  exit 1
fi

# Detect the prohibited pattern: c.score or candidate.score set to rrf_scores value
# e.g. c.score = rrf_scores[c.skill_id]
if grep -qE '\.score\s*=\s*rrf_scores\[|\.score\s*=\s*1\.0\s*/\s*\(|\.score\s*=\s*rrf_val' "$file"; then
  echo "FAIL: public .score field is set to the RRF artifact value — must use semantic_score instead"
  exit 1
fi

# Verify the correct pattern: .score is set to semantic_score (or equivalent pre-fusion relevance)
has_semantic_score_assignment=0
if grep -qE '\.score\s*=\s*.*semantic_score|\.score\s*=\s*c\.semantic|score\s*=\s*candidate\.semantic' "$file"; then
  has_semantic_score_assignment=1
fi

# Also accept: fusion_rank_score used for sorting, score preserved
if grep -qE 'fusion_rank_score|sort.*rrf|rrf.*sort|key.*rrf_scores' "$file"; then
  # RRF used only for sorting — check that score is not also set to RRF
  if ! grep -qE '\.score\s*=\s*rrf_scores\[' "$file"; then
    has_semantic_score_assignment=1
  fi
fi

if [ "$has_semantic_score_assignment" -eq 0 ]; then
  echo "FAIL: .score field is not assigned from semantic_score — public score may still be RRF artifact or unset"
  exit 1
fi

echo "PASS: .score is assigned from semantic relevance (not the RRF artifact); RRF used only for ordering"
exit 0
