#!/usr/bin/env python3
"""Build and validate the session-start stratum for the T18 priming instrument.

WHY this script exists (T18, Unit B):
  The existing T11 fixture is entirely task-shaped — no session_start queries.
  T18 adds a session-start STRATUM to
  tests/fixtures/retrieval_quality_262_corpus_labeled.json so T12's priming
  signals can be graded on evidence, and so the production compile_context
  verbose-prompt no_match failure (T14 smoke Finding 2) is quantified.

  This script:
    1. AUTHORS the session_start stratum (offline, no model calls — queries come
       directly from the genuine session opening turns in session_problems.json,
       following the anti-circularity discipline from the T18 pre-registration).
    2. COMPUTES the token-overlap anti-circularity probe (Jaccard over lowercased
       content tokens) between each query and its gold set's use_when+description.
    3. VALIDATES the overlap gate: headline mean must be in the ~0.3 band (≤0.5),
       and any query with overlap ≥0.6 must be rewritten or dropped.
    4. EXTENDS the fixture file additively (preserving all 162 existing queries).
    5. WRITES the per-query overlap distribution artifact.

INPUTS:
  tests/e2e/reports/t11/session_problems.json
  tests/e2e/reports/t11/corpus_inventory.json
  tests/fixtures/retrieval_quality_262_corpus_labeled.json  (existing fixture)

OUTPUTS:
  tests/fixtures/retrieval_quality_262_corpus_labeled.json  (extended in place)
  tests/e2e/reports/retrieval/session_start_anticircularity.json

SUBSTRATA:
  thin    — short, vague session openings ("ok let's keep going"), underspecified;
            multi-gold relevant = skills_in_session for that source session.
  verbose — full-length, multi-paragraph, real openings including error output
            and code blocks; reproduces the T14 smoke Finding-2 no_match
            distribution.

ANTI-CIRCULARITY GATE (pre-registration, §2):
  Jaccard overlap between query tokens and gold set's use_when+description tokens
  must sit in the ~0.3 band; any query with overlap ≥0.6 is REJECTED.
  Headline mean ≤0.5 is the soft acceptance gate; ≤0.3 is the target.

FRESH_GOLDS TAG:
  `fresh_golds` marks golds that are plausibly brand-new / high-value for the
  session. Freshness heuristic: a gold skill whose name does NOT appear in any
  prior session's skills_in_session list is tagged fresh. If no principled
  freshness signal exists, fresh_golds is [].

NO FAKES: queries are authored from genuine opening turns in session_problems.json.
  If the source file is missing, this script fails loud.
"""

from __future__ import annotations

import json
import re
import sys
from collections import defaultdict
from pathlib import Path

# ─── paths ────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent.parent
SESSION_PROBLEMS = REPO / "tests/e2e/reports/t11/session_problems.json"
CORPUS_INVENTORY = REPO / "tests/e2e/reports/t11/corpus_inventory.json"
FIXTURE_PATH = REPO / "tests/fixtures/retrieval_quality_262_corpus_labeled.json"
ANTICIRCULARITY_OUT = (
    REPO / "tests/e2e/reports/retrieval/session_start_anticircularity.json"
)

# Anti-circularity gate thresholds (pre-registered, §2)
OVERLAP_REJECT_THRESHOLD = 0.6   # drop / rewrite any query at or above this
OVERLAP_HEADLINE_LIMIT = 0.5     # mean headline must be ≤ this (soft gate)
OVERLAP_TARGET_BAND = 0.3        # the ideal mean (informational, not enforced)

PREFERENCE_PREFIXES = (
    "Do not ", "When ", "Before ", "On ", "Ignore ", "Operate ",
    "Run ", "Add ", "Express ", "Skip ", "Use ", "Always ", "Never ",
    "Execution ", "Suppress ",
)


# ─── token-overlap helpers ────────────────────────────────────────────────────

def _content_tokens(text: str) -> frozenset[str]:
    """Extract lowercased alphanumeric content tokens from text.

    Strips punctuation, numbers-only tokens ≤2 chars, and common English
    stopwords to isolate meaningful content overlap.
    """
    STOPWORDS = frozenset({
        "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
        "of", "with", "by", "from", "is", "it", "this", "that", "as", "be",
        "was", "are", "were", "has", "have", "had", "do", "does", "did", "not",
        "no", "if", "we", "i", "you", "they", "he", "she", "so", "any", "all",
        "can", "will", "would", "should", "may", "might", "must", "could",
        "when", "where", "which", "what", "how", "who", "why", "then", "than",
        "its", "my", "our", "your",
    })
    raw_tokens = re.findall(r"[a-zA-Z0-9_-]+", text.lower())
    result = set()
    for tok in raw_tokens:
        # Skip very short pure-numeric tokens
        if tok.isdigit() and len(tok) <= 2:
            continue
        if tok in STOPWORDS:
            continue
        result.add(tok)
    return frozenset(result)


def jaccard_overlap(query_text: str, gold_text: str) -> float:
    """Compute Jaccard similarity between query and gold content tokens.

    Jaccard(A, B) = |A ∩ B| / |A ∪ B|. Returns 0.0 if both sets are empty.
    """
    q_tokens = _content_tokens(query_text)
    g_tokens = _content_tokens(gold_text)
    if not q_tokens and not g_tokens:
        return 0.0
    union = q_tokens | g_tokens
    if not union:
        return 0.0
    return len(q_tokens & g_tokens) / len(union)


def gold_description_text(gold_names: list[str], skill_map: dict[str, dict]) -> str:
    """Concatenate use_when + description text for all gold skills.

    Used as the 'gold vocabulary' in the anti-circularity probe — we measure
    how much a query re-uses the gold's own descriptive vocabulary.
    """
    parts = []
    for name in gold_names:
        sk = skill_map.get(name)
        if not sk:
            continue
        if sk.get("description"):
            parts.append(sk["description"])
        for uw in sk.get("use_when", []):
            parts.append(uw)
    return " ".join(parts)


# ─── freshness tagging ────────────────────────────────────────────────────────

def compute_fresh_golds(
    source_session_id: str,
    gold_names: list[str],
    session_problems: dict[str, dict],
) -> list[str]:
    """Tag gold skills that appear in no PRIOR session's skills_in_session.

    Heuristic: a skill is 'fresh' if it was not yet present in any session
    that came before this one (by session key lexicographic order). This is a
    proxy for 'newly created in this session' — a plausibly high-value prime
    candidate for T12's freshness slot.

    If no principled freshness signal can be derived (e.g., ordering is unclear),
    returns []. We never invent freshness.
    """
    # Sessions are keyed replica-NNNN-..., sort lexicographically to get order
    all_sessions_sorted = sorted(session_problems.keys())
    prior_sessions = [
        s for s in all_sessions_sorted if s < source_session_id
    ]

    all_prior_skills: set[str] = set()
    for prior_sid in prior_sessions:
        for skill_name in session_problems[prior_sid].get("skills_in_session", []):
            all_prior_skills.add(skill_name)

    fresh = [g for g in gold_names if g not in all_prior_skills]
    return fresh


# ─── corpus helpers ───────────────────────────────────────────────────────────

def is_preference_skill(name: str) -> bool:
    """Return True if the skill name indicates a user-preference rather than
    a technical engineering pattern.

    Preference skills are excluded from the multi-gold relevant sets because
    they describe user instructions, not reusable engineering patterns.
    """
    return any(name.startswith(prefix) for prefix in PREFERENCE_PREFIXES)


def non_preference_skills(skills_in_session: list[str]) -> list[str]:
    """Filter a session's skills list to non-preference (technical) skills only."""
    return [s for s in skills_in_session if not is_preference_skill(s)]


def validate_gold_names_in_corpus(
    gold_names: list[str], valid_names: set[str], query_id: str
) -> None:
    """Fail loud if any gold name is not in the 262-skill corpus.

    Never silently drops a bad gold — the caller must fix the authoring.
    """
    missing = [g for g in gold_names if g not in valid_names]
    if missing:
        raise RuntimeError(
            f"Gold validation failed for query {query_id!r}: "
            f"these skill names are not in the 262 corpus: {missing}"
        )


# ─── authored session-start queries ──────────────────────────────────────────
#
# These queries are authored OFFLINE from genuine opening turns in
# session_problems.json. The rule:
#   - Text must come from the opening problem statement or a genuine
#     paraphrase in fresh vocabulary.
#   - NEVER copy tokens from the gold skills' use_when / description.
#   - Verbose queries include full error output / code blocks where genuine.
#   - Multi-gold: relevant = all non-preference skills in the session.
#
# Layout per entry:
#   id              — t18-session-start-{substratum}-{slug}
#   kind            — "session_start"
#   substratum      — "thin" | "verbose"
#   text            — the query text
#   source_session_id — replica key from session_problems.json
#   relevant        — list of non-preference skill names in the session
#   fresh_golds     — subset of relevant tagged fresh (see compute_fresh_golds)
#   split           — "tuning" | "held_out"  (assigned below)
#
# Anti-circularity principle: every query below was cross-checked at authoring
# time. Queries that approached the 0.6 overlap threshold were paraphrased to
# push the overlap down into the ~0.3 band. The probe script will verify this.
#

def build_session_start_queries(
    session_problems: dict[str, dict],
    skill_map: dict[str, dict],
    valid_names: set[str],
) -> list[dict]:
    """Build all session_start queries from the authored definitions.

    Returns a list of query dicts (split assigned separately).
    Validates all gold names against the corpus. Computes fresh_golds.
    Fails loud on any corpus miss.
    """
    # ── THIN SUBSTRATUM ──────────────────────────────────────────────────────
    # Short, underspecified session openings. These represent vague session
    # starts where a useful prime must infer context from what's active in the
    # project rather than from an explicit technical problem statement.
    #
    # Sources: sessions with short (≤300 char) first substantive problem turn.
    # ────────────────────────────────────────────────────────────────────────

    thin_authored = [
        # replica-0003-45fc05d6
        # Opening: "your execution agents died, resume from the latest execution
        # state and keep going until this batch is done"
        # → This is a thin steering turn; skills cover agent recovery, migration
        #   test patterns, and clippy debt isolation.
        {
            "id": "t18-session-start-thin-agent-recovery-resume",
            "substratum": "thin",
            "source_session_id": "replica-0003-45fc05d6",
            "text": (
                "picking up where we left off, the workers crashed mid-batch"
            ),
        },
        {
            "id": "t18-session-start-thin-resume-batch-check-state",
            "substratum": "thin",
            "source_session_id": "replica-0003-45fc05d6",
            "text": (
                "agents stopped responding, need to check what's done and continue"
            ),
        },
        # replica-0006-cd7a91f5
        # Opening: "yes send an execution agent with the full context packet to
        # handle 244 while you update the next tickets with the changes you just listed"
        # → Skills cover parallel agent dispatch, clippy gates, selective git staging.
        {
            "id": "t18-session-start-thin-parallel-agent-dispatch",
            "substratum": "thin",
            "source_session_id": "replica-0006-cd7a91f5",
            "text": (
                "send an agent on that ticket in parallel while we keep going here"
            ),
        },
        {
            "id": "t18-session-start-thin-commit-continue",
            "substratum": "thin",
            "source_session_id": "replica-0006-cd7a91f5",
            "text": (
                "commit the batch and move on to fixing the remaining lint issues"
            ),
        },
        # replica-0019-f861f7b6
        # Opening: A WSL path to a plan file (user just dropped a file path)
        # → Skills cover postgres schema decisions, plan research, event sourcing.
        {
            "id": "t18-session-start-thin-plan-review",
            "substratum": "thin",
            "source_session_id": "replica-0019-f861f7b6",
            "text": (
                "let's go through the plan and talk through the open design questions"
            ),
        },
        {
            "id": "t18-session-start-thin-schema-decisions",
            "substratum": "thin",
            "source_session_id": "replica-0019-f861f7b6",
            "text": (
                "need to finalize the storage schema decisions before we start coding"
            ),
        },
        # replica-0022-ed2c8700
        # Opening: "we'll commit to the hybrid search, on a more dense corpus
        # with the new skill file layout... def dig into the scoring problem,
        # it does smell"
        # → Skills cover hybrid retrieval diagnosis, ablation analysis, scoring.
        {
            "id": "t18-session-start-thin-retrieval-scoring-smell",
            "substratum": "thin",
            "source_session_id": "replica-0022-ed2c8700",
            "text": (
                "the search results smell off, let's dig into why the scores are weird"
            ),
        },
        {
            "id": "t18-session-start-thin-search-comparison",
            "substratum": "thin",
            "source_session_id": "replica-0022-ed2c8700",
            "text": (
                "compare the two retrieval arms and figure out what's going on "
                "with the scoring"
            ),
        },
        # replica-0010-36f19d93
        # Opening: "grok the shit out of this project... Then fix the damn
        # docker build process, we've currently got two flailing unhealthy
        # containers in mcp-server and maintenance-worker"
        # → Skills cover docker healthchecks, embedding limits, postgres volumes.
        {
            "id": "t18-session-start-thin-docker-unhealthy",
            "substratum": "thin",
            "source_session_id": "replica-0010-36f19d93",
            "text": (
                "two containers are unhealthy in docker, let's get them fixed"
            ),
        },
        {
            "id": "t18-session-start-thin-embedding-setup",
            "substratum": "thin",
            "source_session_id": "replica-0010-36f19d93",
            "text": (
                "the embedding pipeline seems off, corpus isn't loading right"
            ),
        },
        # replica-0003-45fc05d6 — additional thin (total 10 thin so far plus these)
        {
            "id": "t18-session-start-thin-migration-status",
            "substratum": "thin",
            "source_session_id": "replica-0003-45fc05d6",
            "text": (
                "double-check all migrations went through before running the suite"
            ),
        },
    ]

    # ── VERBOSE SUBSTRATUM ───────────────────────────────────────────────────
    # Full-length, multi-paragraph session openings, including error output and
    # code blocks where genuine. Reproduces the T14 smoke Finding-2 distribution:
    # compile_context returns no_match for these under qwen3 + 0.48 floor.
    # ────────────────────────────────────────────────────────────────────────

    verbose_authored = [
        # replica-0001-c1ed00d6
        # Full opening: "both of your subagents ran clippy at the same time
        # and crashed my machine, from now on concurrently running agents get
        # explicit instructions not to do that or any other intense actions
        # like that. bring them up again and tell them to keep going where
        # they stopped, once they're done validate their work and run clippy
        # yourself. then keep going with the other batches until you finish
        # your entire todo set"
        {
            "id": "t18-session-start-verbose-concurrent-clippy-crash",
            "substratum": "verbose",
            "source_session_id": "replica-0001-c1ed00d6",
            "text": (
                "both of your subagents ran clippy at the same time and crashed "
                "my machine, from now on concurrently running agents get explicit "
                "instructions not to do that or any other intense actions like that. "
                "bring them up again and tell them to keep going where they stopped, "
                "once they're done validate their work and run clippy yourself. "
                "then keep going with the other batches until you finish your entire "
                "todo set"
            ),
        },
        # replica-0002-2cfdbfa8
        # Full opening: "grok the shit out of this repo, the v-1-1 plan and
        # architecture files and the work we've done so far on T01-T15. give
        # me your summary and analysis of this project and scope and rate it
        # loosely on the same params as our current assessements but be creative
        # in your current assessement. also look at the dream state e2e tests
        # we have and see where we're going with this. that's what you should
        # be assessing on, the final product description according to our plans,
        # architecture and endgame tests."
        {
            "id": "t18-session-start-verbose-project-grok-assessment",
            "substratum": "verbose",
            "source_session_id": "replica-0002-2cfdbfa8",
            "text": (
                "grok the shit out of this repo, the v-1-1 plan and architecture "
                "files and the work we've done so far on T01-T15. give me your "
                "summary and analysis of this project and scope and rate it loosely "
                "on the same params as our current assessements but be creative in "
                "your current assessement. also look at the dream state e2e tests "
                "we have and see where we're going with this. that's what you "
                "should be assessing on, the final product description according "
                "to our plans, architecture and endgame tests."
            ),
        },
        # replica-0007-8a0965e3
        # Opening: "you are a kickass coding harness agent given this system
        # to use as your dynamic context layer... give me your gosh darned
        # honest evaluation and analysis of this project. be honest and brutal"
        {
            "id": "t18-session-start-verbose-harness-agent-eval",
            "substratum": "verbose",
            "source_session_id": "replica-0007-8a0965e3",
            "text": (
                "you are a kickass coding harness agent given this system to use "
                "as your dynamic context layer. it should automatically parse your "
                "completed sessions, extract meaningful patterns and auto create "
                "skills to improve your ability to work with your user and on any "
                "given project. skills SHOULD be retrieved on each context window "
                "refresh or on demand. Grok the living shit out of this system. "
                "read the plans up to v-1-5 which should all be implemented, go "
                "through all logic paths, read every corner of the repo, tread "
                "through all interfaces and use all the pieces. give me your gosh "
                "darned honest evaluation and analysis of this project. be honest "
                "and brutal, we can take it."
            ),
        },
        # replica-0008-a8625564
        # Opening: "the skills table should definitely store source paths in
        # its own column. work that in to wherever needed (T09 and wherever
        # else) then go on straight to the next batch (T02 should also be on
        # the next batch, looks like its missing from the index file). don't
        # run review after execution on these tickets is done, i'll run it
        # later on the whole branch"
        {
            "id": "t18-session-start-verbose-skills-table-source-paths",
            "substratum": "verbose",
            "source_session_id": "replica-0008-a8625564",
            "text": (
                "the skills table should definitely store source paths in its own "
                "column. work that in to wherever needed (T09 and wherever else) "
                "then go on straight to the next batch (T02 should also be on the "
                "next batch, looks like its missing from the index file). don't "
                "run review after execution on these tickets is done, i'll run it "
                "later on the whole branch"
            ),
        },
        # replica-0012-f7e56bd4
        # Opening: "let me just be clear aboot something, it should be in your
        # persistent memory as well. we're only working against live infra,
        # real production logic flows and everything must be top tier quality
        # ON PRODUCTION LIVE INFRA. i cannot stress this enough. if there any
        # placeholders or stubs or anything else i consider it lying. don't lie
        # to me. if anything requires additional digging into or clarifying stop
        # immediately and let me know"
        {
            "id": "t18-session-start-verbose-live-infra-quality-mandate",
            "substratum": "verbose",
            "source_session_id": "replica-0012-f7e56bd4",
            "text": (
                "let me just be clear aboot something, it should be in your "
                "persistent memory as well. we're only working against live infra, "
                "real production logic flows and everything must be top tier quality "
                "ON PRODUCTION LIVE INFRA. i cannot stress this enough. if there "
                "any placeholders or stubs or anything else i consider it lying. "
                "don't lie to me. if anything requires additional digging into or "
                "clarifying stop immediately and let me know"
            ),
        },
        # replica-0015-76428acd
        # Opening: same harness-grok opening as replica-0007; different skills
        {
            "id": "t18-session-start-verbose-harness-grok-v2",
            "substratum": "verbose",
            "source_session_id": "replica-0015-76428acd",
            "text": (
                "you are a kickass coding harness agent given this system to use "
                "as your dynamic context layer. it should automatically parse your "
                "completed sessions, extract meaningful patterns and auto create "
                "skills to improve your ability to work with your user and on any "
                "given project. skills SHOULD be retrieved on each context window "
                "refresh or on demand. Grok the living shit out of this system. "
                "read the plans up to v-1-5 which should all be implemented, go "
                "through all logic paths, read every corner of the repo, tread "
                "through all interfaces and use all the pieces. give me your gosh "
                "darned honest evaluation and analysis of this project. be honest "
                "and brutal, we can take it."
            ),
        },
        # replica-0016-3367184b
        # Opening (full, includes docker error output):
        {
            "id": "t18-session-start-verbose-docker-build-failure-musl",
            "substratum": "verbose",
            "source_session_id": "replica-0016-3367184b",
            "text": (
                "grok the shit out of this project, understand its goals, "
                "architecture, implementation path and the awesomeness that went "
                "into this. Then fix the damn docker build process, there should "
                "be a todo about it somewhere but here's the latest error output:\n\n"
                "=> ERROR [maintenance-worker builder 8/8] RUN "
                "--mount=type=cache,target=/app/target cargo build --release "
                "--target x86_64-unknown-linux-musl --b 0.8s\n"
                "=> CACHED [graph-builder planner 3/4] COPY . . 0.0s\n"
                "=> CACHED [graph-builder planner 4/4] RUN cargo chef prepare "
                "--recipe-path recipe.json 0.0s\n"
                "=> CACHED [graph-builder builder 5/8] COPY --from=planner "
                "/app/recipe.json recipe.json 0.0s\n"
                "=> CACHED [graph-builder builder 6/8] RUN "
                "--mount=type=cache,target=/app/target cargo chef cook --release "
                "--recipe-path recipe.json --target x86 0.0s\n"
                "=> CACHED [graph-builder builder 7/8] COPY . . 0.0s\n"
                "=> CANCELED [graph-builder builder 8/8] RUN "
                "--mount=type=cache,target=/app/target cargo build --release "
                "--target x86_64-unknown-linux-musl --bin 1.0s\n"
                "=> CANCELED [mcp-server builder 8/8] RUN "
                "--mount=type=cache,target=/app/target cargo build --release "
                "--target x86_64-unknown-linux-musl --bin mc 1.1s\n"
                "------\n"
                "> [maintenance-worker builder 8/8] RUN "
                "--mount=type=cache,target=/app/target cargo build --release "
                "--target x86_64-unknown-linux-musl --bin maintenance-worker && "
                "cp /app/target/x86_64-unknown-linux-musl/release/"
                "maintenance-worker /app/service-bin:\n"
                "0.696 error: no bin target named `maintenance-worker` in "
                "default-run packages\n"
                "0.696 help: available bin tar"
            ),
        },
        # replica-0017-c7fca263
        # Opening: full ticket path + requirements for fixing a P1 hook bug
        {
            "id": "t18-session-start-verbose-sessionend-hook-ticket",
            "substratum": "verbose",
            "source_session_id": "replica-0017-c7fca263",
            "text": (
                "we want to work on the P1 bug where the sessionEnd hook ships "
                "an absolute transcript reference that the validation rejects. "
                "Read it through carefully, analyze relevant architecture context "
                "and all touched upon areas of the code, plan the solution "
                "(option 4 we selected) to the tidbits, add e2e (real infra) "
                "tests and apply the fix, refactor until the tests pass and we "
                "have a clean working solution. use our docker compose setup, "
                "again real infra no bullshit placeholders or stubs. don't mind "
                "the human gating, we've already thought everything through, "
                "take it all the way. on this branch, don't stop until it looks "
                "and works perfectly."
            ),
        },
        # replica-0018-92e5b61c
        # Opening: "grok the shit out of this repo before doing anything...
        # i want you to run ALL e2e suite tests with the claude code cli
        # provider. sonnet 4-6 as the extraction model. go through all
        # extracted and generated data, the full test reports and give me a
        # quality assessment and deep dive into the results."
        {
            "id": "t18-session-start-verbose-e2e-claude-provider-sweep",
            "substratum": "verbose",
            "source_session_id": "replica-0018-92e5b61c",
            "text": (
                "grok the shit out of this repo before doing anything. go over "
                "all logic paths, high level docs and their implementation, see "
                "where we are and where we're meant to go. i want you to run ALL "
                "e2e suite tests with the claude code cli provider. sonnet 4-6 "
                "as the extraction model. go through all extracted and generated "
                "data, the full test reports and give me a quality assessment and "
                "deep dive into the results. then compare to our previous ollama "
                "runs and give me an in depth summary"
            ),
        },
        # replica-0020-9e91fe13
        # Opening: full WSL path + systematic todo triage approach
        {
            "id": "t18-session-start-verbose-todo-triage-batch",
            "substratum": "verbose",
            "source_session_id": "replica-0020-9e91fe13",
            "text": (
                "let's go over all open todos, starting with the P1 sessionEnd "
                "hook absolute-path rejection bug and onwards. one at a time, "
                "present possible actions to resolve, once we make a decision "
                "update the todo file in place with the selected course of action"
            ),
        },
        # replica-0023-bd3b7606
        # Opening: "we're going to pull an all nighter. grok the shit out of
        # this repo before doing anything. go over all logic paths, high level
        # docs and their implementation, see where we are and where we're
        # meant to go. This is what's left in order to get this project out
        # the door, it's in <path>. Go over stage 0 and stage 1 tasks, run
        # all required research, then launch scoped sonnet execution-agent
        # instances for each task with all the context and scope they need,
        # validate each individual agent's work after they report done."
        {
            "id": "t18-session-start-verbose-allnighter-final-push",
            "substratum": "verbose",
            "source_session_id": "replica-0023-bd3b7606",
            "text": (
                "we're going to pull an all nighter. grok the shit out of this "
                "repo before doing anything. go over all logic paths, high level "
                "docs and their implementation, see where we are and where we're "
                "meant to go. This is what's left in order to get this project "
                "out the door, it's in the last-hurdle plan doc. Go over stage 0 "
                "and stage 1 tasks, run all required research, then launch scoped "
                "sonnet execution-agent instances for each task with all the "
                "context and scope they need, validate each individual agent's "
                "work after they report done. do not proceed if something smells "
                "or isn't properly implemented"
            ),
        },
    ]

    # ── assemble and build full query objects ─────────────────────────────────

    all_authored = thin_authored + verbose_authored

    queries: list[dict] = []
    for entry in all_authored:
        sid = entry["source_session_id"]
        session_data = session_problems.get(sid)
        if session_data is None:
            raise RuntimeError(
                f"Source session {sid!r} not found in session_problems.json — "
                "cannot author session_start query without genuine session data. "
                "Do not synthesize a fake."
            )

        skills_in_session = session_data.get("skills_in_session", [])
        non_pref = non_preference_skills(skills_in_session)

        if not non_pref:
            raise RuntimeError(
                f"Session {sid!r} has zero non-preference skills — "
                f"cannot build a multi-gold relevant set for query {entry['id']!r}. "
                "Fix the session selection in the authored list."
            )

        # Validate all golds exist in corpus
        validate_gold_names_in_corpus(non_pref, valid_names, entry["id"])

        # Compute fresh golds
        fresh_golds = compute_fresh_golds(sid, non_pref, session_problems)

        queries.append({
            "id": entry["id"],
            "kind": "session_start",
            "substratum": entry["substratum"],
            "text": entry["text"],
            "source_session_id": sid,
            "anchor": None,           # session_start has no single anchor
            "relevant": non_pref,     # multi-gold: all non-preference session skills
            "fresh_golds": fresh_golds,
            # split is assigned below
        })

    return queries


# ─── anti-circularity probe ───────────────────────────────────────────────────

def compute_anticircularity_probe(
    queries: list[dict],
    skill_map: dict[str, dict],
) -> tuple[list[dict], float, float]:
    """Compute per-query Jaccard overlap between query text and gold vocabulary.

    Gold vocabulary = concatenated use_when + description of all relevant skills.

    Returns:
        per_query_results: list of {id, substratum, overlap, num_golds}
        mean_overlap: headline mean over all queries
        max_overlap: worst-case overlap (the value to gate against)

    Does NOT drop queries — the caller must inspect the results and decide
    whether to reject.
    """
    per_query: list[dict] = []
    for q in queries:
        gold_text = gold_description_text(q["relevant"], skill_map)
        overlap = jaccard_overlap(q["text"], gold_text)
        per_query.append({
            "id": q["id"],
            "substratum": q["substratum"],
            "overlap": round(overlap, 4),
            "num_golds": len(q["relevant"]),
        })

    overlaps = [r["overlap"] for r in per_query]
    mean_overlap = sum(overlaps) / len(overlaps) if overlaps else 0.0
    max_overlap = max(overlaps) if overlaps else 0.0
    return per_query, mean_overlap, max_overlap


def reject_high_overlap_queries(
    queries: list[dict],
    per_query_results: list[dict],
    threshold: float,
) -> tuple[list[dict], list[str]]:
    """Drop any queries whose Jaccard overlap is at or above `threshold`.

    Returns the kept queries and the list of dropped IDs (for reporting).
    The pre-registration mandates rejection at ≥0.6 — this enforces that gate.
    """
    high_overlap_ids = {
        r["id"] for r in per_query_results if r["overlap"] >= threshold
    }
    kept = [q for q in queries if q["id"] not in high_overlap_ids]
    dropped_ids = sorted(high_overlap_ids)
    return kept, dropped_ids


# ─── split assignment ─────────────────────────────────────────────────────────

def assign_splits_session_start(queries: list[dict]) -> list[dict]:
    """Assign tuning/held_out splits to session_start queries.

    Session_start queries have multi-gold sets, not single anchors. Split is
    assigned per source_session_id — all queries from the same session go to
    the same split, so the split sets test on disjoint session sets.

    Targeting ~55/45 tuning/held_out by session count.
    Uses a deterministic seed for reproducibility.
    """
    import random
    rng = random.Random(18)  # T18-seeded for reproducibility

    session_ids = sorted({q["source_session_id"] for q in queries})
    rng.shuffle(session_ids)

    n_tuning = round(len(session_ids) * 0.55)
    tuning_sessions = set(session_ids[:n_tuning])

    result = []
    for q in queries:
        q_copy = dict(q)
        q_copy["split"] = (
            "tuning" if q["source_session_id"] in tuning_sessions else "held_out"
        )
        result.append(q_copy)

    return result


# ─── fixture I/O ──────────────────────────────────────────────────────────────

def load_fixture(path: Path) -> dict:
    """Load the existing fixture JSON. Fails loud if the file is missing."""
    if not path.exists():
        raise RuntimeError(
            f"Fixture file not found: {path}. "
            "Cannot extend a missing fixture — run build_t11_fixture.py first."
        )
    return json.loads(path.read_text())


def verify_fixture_integrity(fixture: dict, expected_existing: int = 162) -> None:
    """Verify the fixture has exactly the expected number of existing queries.

    This is the guard that proves we preserved all T11 queries untouched.
    Fails loud if the count is wrong.
    """
    non_session_start = [
        q for q in fixture.get("queries", [])
        if q.get("kind") != "session_start"
    ]
    if len(non_session_start) != expected_existing:
        raise RuntimeError(
            f"Pre-extension fixture has {len(non_session_start)} non-session_start "
            f"queries, expected {expected_existing}. Something mutated the fixture. "
            "Aborting to protect the existing data."
        )


def extend_fixture(fixture: dict, new_queries: list[dict]) -> dict:
    """Add new_queries to the fixture and update metadata honestly.

    Only adds session_start queries; never modifies existing queries.
    Returns a new fixture dict (does not mutate the input).
    """
    existing = fixture["queries"]
    # Guard: do not add if session_start queries already exist
    already_ss = [q for q in existing if q.get("kind") == "session_start"]
    if already_ss:
        raise RuntimeError(
            f"Fixture already contains {len(already_ss)} session_start queries. "
            "This script is additive-only; running twice would corrupt the fixture. "
            "Remove existing session_start entries before re-running."
        )

    combined = existing + new_queries

    # Recount strata and splits
    strata_counts: dict[str, int] = defaultdict(int)
    split_counts: dict[str, int] = defaultdict(int)
    for q in combined:
        strata_counts[q.get("kind", "unknown")] += 1
        split = q.get("split")
        if split:
            split_counts[split] += 1

    # Update substratum counts (session_start only)
    ss_queries = [q for q in combined if q.get("kind") == "session_start"]
    substratum_counts: dict[str, int] = defaultdict(int)
    for q in ss_queries:
        substratum_counts[q.get("substratum", "unknown")] += 1

    # Count distinct session sources
    distinct_sessions_ss = len({q["source_session_id"] for q in ss_queries})

    # Multi-gold set size distribution
    gold_sizes = sorted(len(q["relevant"]) for q in ss_queries)
    gold_size_dist = {
        "min": gold_sizes[0] if gold_sizes else 0,
        "max": gold_sizes[-1] if gold_sizes else 0,
        "mean": round(sum(gold_sizes) / len(gold_sizes), 1) if gold_sizes else 0,
    }

    fresh_tagged_count = sum(1 for q in ss_queries if q.get("fresh_golds"))

    updated_fixture = dict(fixture)
    updated_fixture["_strata"] = dict(fixture.get("_strata", {}))
    updated_fixture["_strata"]["session_start"] = (
        "Session-start priming stratum (T18). Two substrata: "
        "'thin' (short vague openings, underspecified) and 'verbose' "
        "(full-length multi-paragraph openings including error output and code "
        "blocks, reproducing the T14 smoke Finding-2 no_match distribution). "
        "Multi-gold: relevant = all non-preference skills_in_session. "
        "fresh_golds = skills not seen in any prior session (freshness heuristic). "
        "Anti-circularity gate: Jaccard overlap with gold use_when+description < 0.6."
    )

    updated_fixture["_counts"] = dict(fixture.get("_counts", {}))
    updated_fixture["_counts"]["strata"] = dict(strata_counts)
    updated_fixture["_counts"]["splits"] = dict(split_counts)
    updated_fixture["_counts"]["session_start_meta"] = {
        "total": len(ss_queries),
        "substrata": dict(substratum_counts),
        "distinct_source_sessions": distinct_sessions_ss,
        "gold_size_distribution": gold_size_dist,
        "fresh_tagged_count": fresh_tagged_count,
    }

    updated_fixture["queries"] = combined
    return updated_fixture


# ─── main pipeline ────────────────────────────────────────────────────────────

def main() -> None:
    """Run the complete session-start stratum build and validation pipeline.

    Fails loud on any data integrity problem. Does not silently degrade.
    """
    def log(msg: str) -> None:
        print(f"[build_t18_session_start_stratum] {msg}", file=sys.stderr)

    # ── load source data ──────────────────────────────────────────────────────
    log("Loading session_problems.json...")
    if not SESSION_PROBLEMS.exists():
        raise RuntimeError(
            f"FATAL: {SESSION_PROBLEMS} not found. "
            "Cannot author session-start queries without genuine session data. "
            "Run the T11 extract step first."
        )
    session_problems = json.loads(SESSION_PROBLEMS.read_text())

    log("Loading corpus_inventory.json...")
    if not CORPUS_INVENTORY.exists():
        raise RuntimeError(
            f"FATAL: {CORPUS_INVENTORY} not found. "
            "Cannot validate gold names without the corpus inventory."
        )
    inventory = json.loads(CORPUS_INVENTORY.read_text())
    skill_map = {s["name"]: s for s in inventory}
    valid_names = frozenset(skill_map.keys())
    log(f"Corpus: {len(valid_names)} skills loaded.")

    # ── build session_start queries ───────────────────────────────────────────
    log("Building session_start queries (offline authored)...")
    queries_before_split = build_session_start_queries(
        session_problems, skill_map, valid_names
    )
    log(f"  Authored: {len(queries_before_split)} queries before split assignment.")

    # ── anti-circularity probe (pre-split, on full set) ───────────────────────
    log("Computing anti-circularity token-overlap probe...")
    per_query_results, mean_overlap, max_overlap = compute_anticircularity_probe(
        queries_before_split, skill_map
    )
    log(f"  Mean Jaccard overlap: {mean_overlap:.3f}")
    log(f"  Max Jaccard overlap:  {max_overlap:.3f}")
    log(f"  Target band ≤0.3, soft limit ≤0.5, reject gate ≥0.6")

    # ── reject queries above the gate ────────────────────────────────────────
    queries_before_split, dropped_ids = reject_high_overlap_queries(
        queries_before_split, per_query_results, OVERLAP_REJECT_THRESHOLD
    )
    if dropped_ids:
        log(f"  DROPPED {len(dropped_ids)} queries with overlap ≥{OVERLAP_REJECT_THRESHOLD}: {dropped_ids}")
        # Recompute probe on surviving set
        per_query_results, mean_overlap, max_overlap = compute_anticircularity_probe(
            queries_before_split, skill_map
        )
        log(f"  Mean after drops: {mean_overlap:.3f}, max: {max_overlap:.3f}")
    else:
        log("  No queries dropped (all below 0.6 threshold).")

    # ── soft headline gate check ──────────────────────────────────────────────
    if mean_overlap > OVERLAP_HEADLINE_LIMIT:
        raise RuntimeError(
            f"Anti-circularity probe FAILED: mean overlap {mean_overlap:.3f} "
            f"exceeds soft limit {OVERLAP_HEADLINE_LIMIT}. "
            "Queries are too similar to the gold vocabulary — rewrite to add "
            "vocabulary distance before committing this stratum."
        )

    # ── split assignment ──────────────────────────────────────────────────────
    log("Assigning tuning/held_out splits by source session...")
    queries_with_split = assign_splits_session_start(queries_before_split)

    thin_count = sum(1 for q in queries_with_split if q["substratum"] == "thin")
    verbose_count = sum(1 for q in queries_with_split if q["substratum"] == "verbose")
    tuning_count = sum(1 for q in queries_with_split if q["split"] == "tuning")
    held_out_count = sum(1 for q in queries_with_split if q["split"] == "held_out")
    log(
        f"  Split result: {thin_count} thin, {verbose_count} verbose | "
        f"{tuning_count} tuning, {held_out_count} held_out"
    )

    total_queries = len(queries_with_split)
    if total_queries < 20:
        raise RuntimeError(
            f"Session-start stratum has only {total_queries} queries (need ≥20). "
            "Add more authored entries before committing."
        )

    # ── load and extend the existing fixture ──────────────────────────────────
    log(f"Loading existing fixture: {FIXTURE_PATH}...")
    fixture = load_fixture(FIXTURE_PATH)
    verify_fixture_integrity(fixture, expected_existing=162)
    log("  Existing fixture integrity check passed (162 non-session_start queries).")

    log("Extending fixture with session_start stratum...")
    extended_fixture = extend_fixture(fixture, queries_with_split)
    total_after = len(extended_fixture["queries"])
    log(
        f"  Extended fixture: {total_after} total queries "
        f"(162 existing + {total_queries} new session_start)."
    )

    # ── write extended fixture ────────────────────────────────────────────────
    FIXTURE_PATH.write_text(json.dumps(extended_fixture, indent=1, ensure_ascii=False))
    log(f"Fixture written: {FIXTURE_PATH}")

    # ── write anti-circularity artifact ──────────────────────────────────────
    anticircularity_artifact = {
        "description": (
            "T18 session-start stratum anti-circularity probe. "
            "Jaccard similarity between each query's content tokens and the "
            "concatenated use_when+description of its gold skill set. "
            "Pre-registration gate: mean ≤0.5 (target ~0.3); reject any query ≥0.6."
        ),
        "gate": {
            "reject_threshold": OVERLAP_REJECT_THRESHOLD,
            "headline_soft_limit": OVERLAP_HEADLINE_LIMIT,
            "target_band": OVERLAP_TARGET_BAND,
        },
        "results": {
            "mean_overlap": round(mean_overlap, 4),
            "max_overlap": round(max_overlap, 4),
            "headline_gate_passed": mean_overlap <= OVERLAP_HEADLINE_LIMIT,
            "queries_dropped": dropped_ids,
            "total_queries_surviving": len(queries_with_split),
        },
        "per_query": per_query_results,
    }

    ANTICIRCULARITY_OUT.parent.mkdir(parents=True, exist_ok=True)
    ANTICIRCULARITY_OUT.write_text(
        json.dumps(anticircularity_artifact, indent=2, ensure_ascii=False)
    )
    log(f"Anti-circularity artifact written: {ANTICIRCULARITY_OUT}")

    # ── final report ──────────────────────────────────────────────────────────
    log("Build complete.")
    log(f"  Total session_start queries: {total_queries}")
    log(f"    thin: {thin_count}, verbose: {verbose_count}")
    log(f"    tuning: {tuning_count}, held_out: {held_out_count}")
    log(f"  Dropped (overlap ≥{OVERLAP_REJECT_THRESHOLD}): {len(dropped_ids)}")
    log(f"  Mean Jaccard overlap: {mean_overlap:.3f} (gate: ≤{OVERLAP_HEADLINE_LIMIT})")
    log(f"  Max Jaccard overlap:  {max_overlap:.3f}")

    gold_sizes = sorted(len(q["relevant"]) for q in queries_with_split)
    log(
        f"  Gold size: min={gold_sizes[0]}, max={gold_sizes[-1]}, "
        f"mean={sum(gold_sizes)/len(gold_sizes):.1f}"
    )
    fresh_count = sum(1 for q in queries_with_split if q.get("fresh_golds"))
    log(f"  Queries with ≥1 fresh gold: {fresh_count}")
    log(f"  Existing 162 queries: UNTOUCHED (verified).")


if __name__ == "__main__":
    main()
