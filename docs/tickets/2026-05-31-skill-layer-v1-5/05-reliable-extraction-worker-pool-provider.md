---
ticket_id: T05
title: Reliable real extraction — worker-pool correctness + provider disposition
kind: hardening # tracer-bullet | expansion | hardening | infra-track | fix-batch
status: completed # ready | in_progress | blocked | completed
plan_ref: docs/plans/2026-05-31-feat-skill-layer-v1-5-close-the-loop-plan.md
tickets_ref: docs/tickets/2026-05-31-skill-layer-v1-5/index.md
architecture_ref: docs/architecture/2026-05-31-skill-layer-v1-5-close-the-loop-architecture.md
source_packet_ref: "## Execution Slices > Slice 2.2: Reliable real extraction (Ollama) + worker-pool correctness + provider disposition"
feature_home: crates/session-extractor
depends_on: [T04]
dependency_type: hard # none | hard | soft | parallel-safe
serves:
  - SC-V1.5-C (real extraction is reliable)
  - SC-V1.5-F (no production stub paths remain)
files:
  - crates/session-extractor/src/worker_pool.rs
  - crates/session-extractor/src/lib.rs
  - crates/infrastructure/src/extraction/ollama.rs
  - crates/infrastructure/src/extraction/claude.rs
test_command: cargo test -p session-extractor
tdd_mode: inherit
---

# Reliable real extraction — worker-pool correctness + provider disposition

## Serves
- **SC-V1.5-C** — Ollama extraction completes end to end; under a ≥32-job burst every accepted job emits exactly one terminal event and writes/declines `.pending` deterministically.
- **SC-V1.5-F** — kill the dead `_retry_policy` and the `:8080/extract` stub path.
- Plan SC-3; constitution "No stubs".

## Scope
Fix the worker-pool throughput/correctness bugs (the "0/32" cause), consolidate terminal-event ownership, calibrate timeout/model for `granite4:3b`, default the provider to Ollama, and repoint the Claude provider to the real Anthropic Messages API (forced `tool_use`).

- **Owns:** extraction completion guarantees, worker-pool concurrency/timeout/retry, provider disposition.
- **Non-goals:** SkillLens quality scoring, map-reduce extraction, prompt re-design (V2 owns extraction *quality*).

## Scope Fence
V1.5 makes extraction *reliable and real*, not *smarter*. No new compose service/port. No CLI subprocess, no sidecar.

## Acceptance Criteria
- [x] ≥32 parallel jobs: each emits exactly one terminal event; counts reconcile.
- [x] **Throughput root-cause fixed:** replace the single `Arc<Mutex<Receiver>>` (`worker_pool.rs:68`, held across `recv().await`) with an MPMC receiver (`async-channel`/`flume`) so N workers pull concurrently. Remove the now-redundant semaphore. (Unconditional — this is the confirmed "0/32" cause.)
- [x] **Terminal-event ownership consolidated in the dispatch layer:** `execute_job` returns a typed `ExtractionOutcome` and publishes nothing; the worker loop (and the no-pool spawn path) own all three events (completed/failed/timeout). Add a timeout arm to the no-pool path (currently can stall silently).
- [x] **Both retry sites unified:** the dead `_retry_policy` param AND the hardcoded `RetryPolicy{max_attempts:3,…}` in `extract_with_retry` (`lib.rs:407–411`) collapse to one config-sourced policy.
- [x] Default provider is **Ollama**: `EXTRACT_SESSION_PROVIDER` unset ⇒ Ollama (fix `lib.rs:196–197`); empty string no longer silently maps to Claude.
- [x] **Model + timeout calibrated for `gemma4:e4b`:** `OllamaExtractionConfig` default model is `gemma4:e4b` (shipped; bumped from `granite4:3b` post-T05 — see retro-approval `docs/execution-sessions/retro-2026-06-02-model-healthcheck-approval/retro-approval.md`); inner `timeout_ms` realistic for CPU inference (current 1.5s inner vs 30s outer fails every real extraction); document measured single-job p50/p95 on the target host; pool timeout ≥ 1.5× inner.
- [x] **Claude provider = direct Anthropic Messages API:** `ClaudeExtractor` POSTs to `https://api.anthropic.com/v1/messages` (configurable via `ANTHROPIC_BASE_URL`), returns the candidate shape via a forced `tool_use` (tool `input_schema` = candidate schema), keyed by `ANTHROPIC_API_KEY`, model via `EXTRACT_SESSION_MODEL` (default `claude-sonnet-4-6`; bumped from `claude-haiku-4-5` post-T05 — cost increase acknowledged, see retro-approval `docs/execution-sessions/retro-2026-06-02-model-healthcheck-approval/retro-approval.md`). The `:8080/extract` default is deleted. `provider=claude` without `ANTHROPIC_API_KEY` is a loud construct-time error (no silent fallback). Mark the static prompt block with `cache_control: ephemeral`.
- [x] **Local-first preserved on the default path:** Ollama default needs no cloud key; `ANTHROPIC_API_KEY` is read from env, never committed.
- [x] **Opt-in provider contract documented (constitution v2.0.0):** capability catalog / README document how to opt into Claude (`EXTRACT_SESSION_PROVIDER`, `EXTRACT_SESSION_MODEL`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`) and state Ollama is the default. Claude is presented as a first-class option, not a stub/exception.
- [x] **Compose provider is explicit:** `docker-compose.yml` sets `EXTRACT_SESSION_PROVIDER=ollama` so the default deployment's provider is self-evident (no cloud reached by default).
- [x] **Security P1 — strip absolute host paths from `draft_paths` in the `extraction.completed` event** (emitted in `session-extractor/src/lib.rs`): publish scope-relative paths only, never absolute host paths (info-leak in a Redis-published event). Add a test asserting no absolute path appears in the event payload.

## Shared / Global Notes
- **Provider disposition (constitution v2.0.0 — first-class opt-in):** per the 2026-06-01 amendment (todo 105), the skill-extraction LLM provider is user-selectable. Ollama is the default and the only data-plane-local path; Claude (`EXTRACT_SESSION_PROVIDER=claude`) is a **first-class, fully supported** opt-in provider — NOT a stub and NOT a tolerated exception. No waiver is required. Obligations on this ticket: (a) code default is Ollama (AC below); (b) Claude is the real Anthropic Messages API (AC below); (c) `provider=claude` without `ANTHROPIC_API_KEY` fails loudly at construct time; (d) **document the opt-in contract** (`EXTRACT_SESSION_PROVIDER`, `EXTRACT_SESSION_MODEL`, `ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`) in the capability catalog / README so users know how to opt in; (e) set `EXTRACT_SESSION_PROVIDER` explicitly in `docker-compose.yml` (to `ollama`) so the default deployment's provider is self-evident rather than implicit.
- **Security:** the old `:8080/extract` is the graph-builder admin port (confused-deputy/SSRF) — deleting it closes that hazard.
- **Security (jailbreak filter — defense-in-depth, from todo 114 item 5 + todo 093):** the prompt-injection prefix filter in `crates/infrastructure/src/extraction/ollama.rs` (~72-114) uses `starts_with`/case-sensitive checks that miss mid-string and case variants. The real trust boundary is the XML-delimiter escaping (which is correct); treat the prefix filter as defense-in-depth only. Since todo 105 made Ollama the **default** extraction provider, this is now the *active* path — coordinate hardening with **todo 093** (prompt-injection in Ollama extraction). Do not rely on the prefix filter as the primary guard.
- Cross-feature-home: writes touch `crates/infrastructure/src/extraction/{ollama,claude}.rs` (shared extraction adapters) + `crates/session-extractor/` (the feature home). Keep worker-pool orchestration in session-extractor.
- Human-gate: none for code, but the provider env contract must be documented; no secret is committed.

## Local Context
**WHY:** `worker_pool.rs` publishes `extraction.completed` inside `execute_job` only on success; the worker's own success branch emits no event; the 30s timeout is tight for parallel `granite4:3b`; `recv` is serialized behind one `Mutex`; `_retry_policy` is accepted but never used → 0/32 parallel jobs completed. `claude.rs` POSTs to a non-existent `:8080/extract`.

**Open question to surface:** measure granite4:3b p95 on the target host before fixing the timeout numbers — do not hardcode an unmeasured value.

## Parent Refs
- Plan → Slice 2.2; Architecture artifact.
- Source packet: `## Execution Slices > Slice 2.2`.

## Deeper-Dive Refs
- Plan §Deepening Research Insights §2.2 (MPMC receiver; `ExtractionOutcome` ownership; timeout arithmetic; security on `:8080`).
- Plan Ratified Decisions #2 & #3 (Claude = direct Anthropic API; Ollama default).

## Coupling Notes
One unit because the receiver fix, event-ownership consolidation, timeout calibration, and provider disposition are all the single "make extraction reliable and real" outcome — fixing the receiver without consolidating event ownership would still lose terminal events. Hard-depends on T04 (the SessionEnd trigger it backstops). Parallel-safe with T02 in Batch 3 (disjoint files: session-extractor/extraction vs orchestrator/lib/graph-builder).

## Scope Fence — Superseded Note (2026-06-02)

> **The "No CLI subprocess" clause of this ticket's scope fence is superseded for the host-only
> path by ADR-0002 and constitution v2.1.0.**
>
> The original fence (line 39: "No CLI subprocess, no sidecar, no new compose service/port") was
> correct for the `docker compose` container context and remains in force there. Post-ticket
> commits `bcfa9de`, `d8e45f3`, `295bfef` introduced a `claude -p` CLI subprocess provider that
> runs on the developer's host machine — not in the container — where the container-context
> objection (Node + CLI + credential mounting inside the container) does not apply.
>
> The fence clause is superseded ONLY for the host-only deployment path. The compose container
> path is unaffected: `EXTRACT_SESSION_PROVIDER=ollama` remains the compose default; the
> `claude-code` provider must not be set in `docker-compose.yml`.
>
> **Authoritative references:**
> - ADR-0002: `docs/architecture/adr-0002-claude-code-cli-extraction-provider-v1-5.md`
> - Constitution v2.1.0: `docs/constitution.md` (amendment log entry, 2026-06-02)
> - Retro-ticket: `docs/tickets/2026-05-31-skill-layer-v1-5/T05-addendum-claude-code-cli-provider.md`
