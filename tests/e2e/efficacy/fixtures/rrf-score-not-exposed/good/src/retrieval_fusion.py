"""Retrieval fusion — correct: exposes semantic_score, uses RRF only for ordering."""
from dataclasses import dataclass
from typing import List


@dataclass
class ScoredSkill:
    skill_id: str
    name: str
    semantic_score: float
    score: float = 0.0
    fusion_rank_score: float = 0.0


RRF_K = 60


def fuse_and_rank(candidate_lists: List[List[ScoredSkill]]) -> List[ScoredSkill]:
    """
    Fuse multiple ranked candidate lists using RRF, then expose pre-fusion
    semantic relevance as the public score (not the RRF rank artifact).
    """
    rrf_scores: dict = {}
    candidates: dict = {}

    for ranked_list in candidate_lists:
        for rank, candidate in enumerate(ranked_list, start=1):
            rrf_val = 1.0 / (RRF_K + rank)
            rrf_scores[candidate.skill_id] = rrf_scores.get(candidate.skill_id, 0.0) + rrf_val
            candidates[candidate.skill_id] = candidate

    result = sorted(candidates.values(), key=lambda c: rrf_scores[c.skill_id], reverse=True)
    for c in result:
        # Store RRF as internal sort key only — never expose it as the public score
        c.fusion_rank_score = rrf_scores[c.skill_id]
        # Expose the pre-fusion semantic relevance as the public-facing score
        c.score = c.semantic_score
    return result
