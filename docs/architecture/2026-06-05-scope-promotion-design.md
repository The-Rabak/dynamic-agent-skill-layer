---
date: 2026-06-05
status: proposed
deciders:
  - repository-owner (rabak)
related_todos: ["178", "179", "180", "181", "182"]
supersedes: []
---

# Design: Scope Promotion — closing the project↔global gap in the self-growth loop

## Context

The skill layer ships two namespaces — project-specific (multiple, keyed by project root) and global
(one per machine). An end-to-end audit (2026-06-05) found the separation is realised on the **read
side** but is effectively **absent on the write/extract side**.

**Retrieval/storage — separation is real and load-bearing:**
- Two concurrent, scope-partitioned searches (`crates/retrieval/src/dual_scope.rs:43`
  `search_scopes_concurrently`), each gated by a three-way scope filter — scope-type enum + `scope_id`
  string + true path-prefix provenance against `source_paths` (`dual_scope.rs:123`
  `seeded_skill_matches_scope`).
- Asymmetric weighted RRF fusion: `project_weight=1.0`, `global_weight=0.7`
  (`crates/retrieval/src/orchestrator.rs:148`), scope-priority tiebreak Project>Global>Team
  (`crates/retrieval/src/fusion.rs:164`).
- Storage backs it honestly: `skills.scope` CHECK-constrained column + `source_paths TEXT[]`
  (migration 005) for real provenance; graph-builder watches both roots and tags each skill by the
  root it was found under (`crates/graph-builder/src/watcher.rs:325` `scope_for_path`).

**Extraction — separation is degenerate:**
- One scope-blind LLM pass over the canonical contract
  (`crates/infrastructure/src/extraction/prompt_contract.rs:63` `CANONICAL_CONTRACT` — says nothing
  about scope). `ExtractedSkillCandidate` / `ExtractionResult` (`crates/domain/src/types.rs:177`)
  carry **no scope field**. The model never classifies project vs global.
- Scope is decided purely by I/O routing after extraction: `resolve_scope_root`
  (`crates/session-extractor/src/writer.rs:219`) — `repo_path` present → all candidates to project;
  absent → all candidates to the global fallback. One bucket per session. No second run, no
  cross-scope dedupe.
- In practice the shipped hook always sets `repo_path = cwd`
  (`config/claude-code/capture-transcript.sh:51`), so **100% of extracted skills route to project**.
  `extract_session` never grows the global scope. Global is fed only by hand-authored `SKILL.md`,
  and the default global mount points at `${SKILL_GLOBAL_HOST_PATH:-./docs}` — the repo's own docs,
  not a machine-wide store.
- Already acknowledged in-repo as an approximation: `docs/execution-sessions/work-2026-06-01-215338/
  unit-01-t06-record-usage-prior.md:73` — *"scope derived as `repo_path.is_empty() ? "global" :
  "project"` — a V1.5 approximation."*

The infrastructure to **hold** the project/global distinction is solid; the intelligence to
**produce** it at write time does not exist. The self-growth loop only ever grows the project scope.

## The conceptual model: globalness is two different signals

A skill is "global" for one of two reasons, knowable at different times:

1. **Intrinsically general** — the lesson is about a tool/language/ecosystem, not this repo
   ("declare cargo `[[bin]]` explicitly or the binary is named after the package"; "cross-compiling
   Rust to musl needs `musl-tools` for ring/cc-rs"; "fail loud instead of stubbing"). Detectable from
   the **content of one session** — it names no project-local identifiers.
2. **Emergently general** — a pattern that looks project-specific until the **same lesson recurs
   across several unrelated projects**. Knowable only **in aggregate**, never from one session.

This split decides where the work belongs:
- **Emergent generality is structurally a maintenance concern** — it needs cross-project,
  cross-session evidence that only the machine-wide Postgres aggregate can see. Extraction sees one
  transcript and is epistemically incapable of judging recurrence.
- **Global is high-blast-radius** — a global skill leaks into *every* project's retrieval (weight
  0.7). High-blast-radius mutations are exactly what the maintenance worker's propose-only /
  `.pending` / human-gate model exists to protect. Writing straight-to-global from one unreviewed
  extraction bypasses that gate.
- **But intrinsic generality is cheap to detect at extraction**, where the full transcript is still
  in hand — and making a universal lesson wait for cross-project recurrence is needlessly slow (and
  never fires for single-project users).

**Division of responsibility:** extraction captures a cheap *hint*; maintenance owns the
*authoritative* global decision and is the **sole producer** of global proposals.

## Existing substrate in the maintenance worker

The architecture already leans toward "scope decisions live in maintenance" — it implements the
*downward* half of the axis:

- `MergeProposal` already carries `canonical_scope`, `merged_from_scopes`, `merged_from_paths`
  (`crates/maintenance/src/merge.rs:88`); `merged_from_scopes TEXT[]` has existed since migration 001.
- `find_candidates` is explicitly **cross-scope only** — it *skips* same-scope pairs
  (`merge.rs:211` `if left.scope == right.scope { continue }`; doc-comment "Finds cross-scope
  candidates").
- `ScopeSelectionPolicy::PreferProjectThenGlobal` (`merge.rs:110`) resolves which scope a merged pair
  collapses into — and it canonicalises **downward** (toward project).
- A periodic cron drives merge + retire passes (`crates/maintenance/src/cron.rs:65` `tick`), each
  emitting propose-only `.pending` artifacts behind the human rename-to-approve gate.

The missing direction is **promotion upward**: detecting that a lesson is general (intrinsically, or
by recurring across multiple *project* roots — which `find_candidates` currently skips because it
only pairs across scope *types*) and proposing it be lifted to global.

## Options considered

**Option A — Tag at birth (all in extraction).** LLM marks each candidate `project|global|uncertain`;
writer routes global ones straight to the global `.pending` dir.
- Rejected as the authoritative mechanism: single-session myopia over-calls global; false positives
  are far costlier in global (pollute every project) than in project; writes machine-wide state from
  one unreviewed session, bypassing the aggregate judgment and weakening the human gate.

**Option B — Promotion in maintenance (pure form).** Extraction stays 100% scope-blind; a periodic
pass clusters approved project skills across all project scopes and promotes those that recur across
≥N projects.
- Correct epistemics, reuses merge/cluster/propose machinery, keeps the gate. But cold-start: a
  genuinely universal lesson learned once sits in project scope until it recurs, and with a single
  registered project it *never* promotes. Useless on day one for solo-project users.

**Option C — Hybrid (RECOMMENDED).** Extraction records a cheap generality hint but never routes on
it (write path stays honestly project-local). Maintenance owns the decision and promotes via two
paths:
- *Intrinsic path:* extraction hinted "general" **and** a verifier confirms the skill references zero
  project-local identifiers → propose to global from a single occurrence.
- *Evidence path:* the same skill recurs across ≥2 distinct project `source_paths` roots → propose to
  global by recurrence, regardless of hint.
- Both paths emit a global `.pending`; nothing auto-applies.

C is instant for obvious universal lessons, evidence-based for ambiguous ones, and keeps global
behind maintenance + human gate.

## Recommended design (Option C)

### Extraction side — small and honest (todo #178)
Add one advisory field to the canonical contract output: `generality: project | general | uncertain`
plus a one-line rationale, persisted into the `.pending` frontmatter and the skill row.
**Invariant preserved:** the writer still always writes project-local; the hint is *data for
maintenance*, not a routing decision. Global is still born only via a maintenance proposal. No
blast-radius regression to the extraction write path.

### Maintenance side — the real work: a new `promote.rs` pass (todos #179, #180)
- New `PromotionPassRunner` added to `MaintenanceCron::tick` alongside `run_merge_pass` /
  `run_retirement_pass` (`cron.rs:65`); same interval gating; extend `MaintenancePassOutcome` with
  `promotion_proposals`.
- **Read from Postgres, not the filesystem mounts.** The merge runner today loads snapshots by
  walking mounted `scope_roots` (`runtime.rs:286`), which only sees the one mounted project + global.
  The aggregate truth — every project that shares this machine's PG — already lives in the `skills`
  table (`scope='project'` rows across distinct `source_paths`). Querying PG enables cross-project
  recurrence without mounting N filesystems and reuses the existing embeddings for clustering.
- `PromotionProposal { skill_ids, from_scopes, to_scope: Global, evidence: Intrinsic |
  Recurrence{project_count}, .. }`, written as a global `.pending` through the same scope-confined
  writer path that already enforces `ensure_path_is_within_scope_root` (`merge.rs:256`).
- **Do not reuse `PreferProjectThenGlobal`** — that canonicalises downward. Promotion needs its own
  `to_scope: Global` policy. The `merged_from_scopes` column and cross-scope `find_candidates` are
  the right substrate; promotion is the missing inverse direction on the same axis.
- Reuse the `LlmMergeSemanticVerifier` seam (`merge_verifier.rs`) for the intrinsic "references no
  project-local identifiers" check (same provider plumbing, different prompt) and for recurrence
  equivalence between candidate project skills.

### Symmetry — demotion (todo #182)
A global skill whose text references project-local identifiers is a mis-scope; flag it for a
demotion proposal. Cheap, and it cleans up any manual-authoring or Option-A-style mistakes.

## Invariants preserved
- Extraction only ever proposes project-local drafts; global is born only via a maintenance proposal.
- Every scope mutation (promote, demote, merge, retire) is a propose-only `.pending`/`.retired`
  artifact behind the human rename-to-approve gate. **No auto-apply to global, ever.**
- No stubs / fail-loud: the recurrence pass must `log()` its threshold and degrade loudly, never
  silently no-op (see caveats).

## Caveats (must ship with the work)
1. **Cross-project recurrence needs multiple projects** sharing one machine PG. In a single-project
   install only the *intrinsic* path can promote — so the intrinsic path is what makes this useful on
   day one; recurrence makes it smart once the user has several repos. The pass MUST `log()` the
   `N≥2 distinct project roots` threshold and how many it saw, so a single-project install reads as
   "nothing to promote yet," never as a silent success. (todos #179, #180)
2. **The global root still points at the wrong directory.** Default global mount is
   `${SKILL_GLOBAL_HOST_PATH:-./docs}` (the repo's docs) and the hook always sets `repo_path`.
   Promotion gives a real *producer* of global skills, but the global *root* must point at an actual
   machine-wide skill dir (e.g. `~/.claude/skills`), or promotion writes proposals into the wrong
   place. This is a separate compose/config fix that MUST land alongside promotion. (todo #181)
3. **Global is the one scope where auto-apply is unacceptable.** Keep promotion strictly
   propose-only behind the rename-to-approve gate. (enforced across #179, #182)

## Work breakdown (vertical slices)
- **#178** — Extraction generality hint (advisory field; writer stays project-local).
- **#179** — `promote.rs` + `PromotionPassRunner` + cron wiring + intrinsic-path promotion +
  `PromotionProposal` writer (scope-confined, propose-only). Depends on #181.
- **#180** — PG cross-project recurrence query + recurrence-path promotion + threshold logging.
  Depends on #179.
- **#181** — Global-root config fix (point global mount at a real machine-wide dir, not `./docs`).
  Independent; prerequisite for #179 to be useful in prod.
- **#182** — Demotion symmetry (flag mis-scoped global skills). Depends on #179.
