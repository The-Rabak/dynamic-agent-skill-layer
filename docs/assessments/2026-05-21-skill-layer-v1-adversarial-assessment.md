# Adversarial Plan Assessment: Dynamic Agent Skill Layer V1

Date: 2026-05-21  
Assessor: GitHub Copilot CLI

## Executive Verdict

- Idea quality (vision): 7/10
- Architecture quality: 5.8/10
- Daily-use readiness: 4.8/10
- Overall weighted score: 58%

Decision framing: **Re-scope**

## Scope Reviewed

- `docs/architecture/2026-05-21-skill-layer-v1-architecture.md`
- `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md`
- `docs/constitution.md`
- `CONTEXT.md`
- `.github/skills/adversarial-plan-audit/references/audit-rubric.md`
- External references: SkillRAE paper (`arXiv:2605.10114`), Claude Code Skills/Memory/Hooks, GitHub Copilot Memory/Agent Skills/Spaces, AgentSkills, Cline Rules/Memory Bank

## Score Summary

| Principle / Track | Score | Percentage | Status |
|---|---:|---:|---|
| Action parity | 6.5/10 | 65% | ⚠️ Partial |
| Tools as primitives | 5.5/10 | 55% | ⚠️ Partial |
| Context injection | 6.3/10 | 63% | ⚠️ Partial |
| Shared workspace | 5.0/10 | 50% | ⚠️ Partial |
| CRUD completeness | 6.0/10 | 60% | ⚠️ Partial |
| UX/feedback integration | 5.7/10 | 57% | ⚠️ Partial |
| Capability discovery | 6.8/10 | 68% | ⚠️ Partial |
| Prompt-native extensibility | 6.0/10 | 60% | ⚠️ Partial |
| Adversarial risk profile | 4.2/10 | 42% | ❌ Needs Work |
| Market differentiation | 6.0/10 | 60% | ⚠️ Partial |

## What Is Working Excellently

1. The thesis is coherent and differentiated: local-first, human-gated, filesystem-observable skill lifecycle is explicit and constitutionally grounded (`docs/constitution.md:39-69`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:181-186`).
2. The retrieval concept is stronger than typical rules-file systems: multi-level SkillRAE graph, subunits, MMR-then-RRF fusion, and rescue-aware compilation are real differentiators (`CONTEXT.md:19-25`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:62-70`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:460-475`).
3. The plan already names the right hardening classes before implementation: outbox, rebuild locks, TTL warnings, audit log, and event envelopes (`docs/architecture/2026-05-21-skill-layer-v1-architecture.md:116-128`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:617-685`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1148-1196`).

## Core Gaps

1. The plan and architecture are not canonicalized: crate boundaries, ownership, and tool surfaces conflict across sections (`docs/architecture/2026-05-21-skill-layer-v1-architecture.md:35-48`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:62-63`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:85-95`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:131-139`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:790-797`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1022-1026`).
2. Daily-use trust is undercut by ambiguous empty responses, one-shot suppression risk, stale-context coupling, and unresolved transcript ingestion across Docker boundaries (`docs/constitution.md:48-49`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:867`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:884-887`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:122-128`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:280-284`).
3. The target user is a solo power user, but the adoption path is a five-container stack with no concrete day-0 guide or capability catalog yet (`docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:151`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1393-1442`).

## Adversarial Risk Matrix

| Risk | Why It Fails in Practice | Impact | Likelihood | Mitigation |
|---|---|---|---|---|
| Silent empty context | Cold-start and degraded-infra both collapse to "empty," so users cannot tell "no relevant skills" from "system failed" (`docs/constitution.md:48-49`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:535-536`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:886-887`) | High | High | Return `ok/degraded` with reason code and health markers. |
| One-shot suppression after transient failure | The first-prompt suppression rule can consume the only injection chance even if the first attempt degraded or returned empty for infra reasons (`docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:463-475`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:884-887`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:124`) | High | Medium | Suppress only after a successful compile, not after any first invocation. |
| `transcript_path` trust boundary | Session-end extraction still depends on a host path the container may not be able to read safely or consistently (`docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:497-500`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1024`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:283`) | High | High | Replace raw path transport with payload or strict mounted-root contract. |
| Watcher/rename approval flakiness | Approval semantics rely on filesystem renames and watcher correctness; missed rename events turn human approval into non-deterministic behavior (`docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:505-507`, `docs/architecture/2026-05-21-skill-layer-v1-architecture.md:122`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1193-1195`) | Medium | Medium | Add explicit approval audit checkpoints, reconciliation scan, and rename-idempotency guarantees. |
| Stale context from outbox/cache/event coupling | `graph.rebuilt`, cache invalidation, outbox drain, and session state are tightly coupled; any sequencing drift produces stale or duplicate context (`docs/architecture/2026-05-21-skill-layer-v1-architecture.md:118-124`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:722`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1126-1128`, `docs/plans/2026-05-21-feat-skill-layer-v1-plan.md:1259-1262`) | High | Medium | Define one canonical state/invalidations contract and test degraded ordering explicitly. |

## Competitive Positioning

The concept has a real niche, but not a durable moat. The strongest differentiators are the semantic retrieval layer and lifecycle machinery; the weakest parts are setup friction and incumbent overlap.

| Competitor | Core Value | Strength | Weakness | Niche for Project |
|---|---|---|---|---|
| Claude Code Skills + Memory | Native skills, hooks, and memory inside the primary harness | Deep integration and low friction | Lacks explicit SkillRAE-style graph retrieval, subunit retrieval, and human-gated lifecycle | Add semantic retrieval and lifecycle management under Claude Code rather than replacing it |
| GitHub Copilot Memory + Agent Skills | Cloud-managed memory plus AgentSkills-compatible customization | Massive distribution, zero local setup | Cloud-first, weaker local-first/privacy story, no local graph lifecycle | Win with local-first, filesystem-visible, cross-harness retrieval infrastructure |
| Cursor Rules | Lightweight project rules with minimal setup | Very low friction | Flat rules, single-harness, manual curation | Offer cross-harness portability plus semantic ranking |
| Cline Rules + Memory Bank | Offline/open-source rules and manual memory workflow | Works locally and across multiple rule types | Manual upkeep, no semantic graph or automated extraction | Become the retrieval + maintenance backend for MCP-capable open-source harnesses |
| AgentSkills open standard | Shared skill file format across harnesses | Cross-tool compatibility is already standardized | Format only; retrieval, extraction, dedup, and maintenance are unspecified | Position as the semantic/runtime layer on top of the standard, not a competing format |

**External sources:**  
Claude Code Skills: <https://docs.anthropic.com/en/docs/claude-code/skills>  
Claude Code Memory: <https://docs.anthropic.com/en/docs/claude-code/memory>  
Claude Code Hooks: <https://docs.anthropic.com/en/docs/claude-code/hooks>  
GitHub Copilot Memory: <https://docs.github.com/en/copilot/concepts/agents/copilot-memory>  
GitHub Copilot Agent Skills: <https://docs.github.com/en/copilot/concepts/agents/about-agent-skills>  
GitHub Copilot Spaces: <https://docs.github.com/en/copilot/concepts/context/spaces>  
AgentSkills: <https://agentskills.io/>  
Cline Rules: <https://docs.cline.bot/features/cline-rules>  
Cline Memory Bank: <https://docs.cline.bot/features/memory-bank>  

**Commodity areas:** SKILL.md as a format, project/global skill scopes, MCP compatibility, and markdown-based agent instructions are already market-standard.  
**Differentiating areas:** SkillRAE graph retrieval, session-end extraction into SKILL.md drafts, offline graph hygiene, rescue-aware subunit compilation, and strict human-gated mutation flow.

## Naming and Namespace Simplification

The current crate naming scheme repeats the domain in every crate and namespace (`skill-domain`, `skill-infrastructure`, `skill-mcp-server`, `skill-graph-builder`) (`docs/architecture/2026-05-21-skill-layer-v1-architecture.md:37-47`). That prefix is redundant because the repository, workspace, and surrounding docs already establish the domain context. Keeping it everywhere adds noise to ownership tables, import paths, and architectural discussion without adding disambiguation.

**Assessment recommendation:** remove the `skill-` prefix from crate names and namespaces as part of the next canonicalization pass.

| Current | Recommended |
|---|---|
| `skill-domain` | `domain` |
| `skill-infrastructure` | `infrastructure` |
| `skill-mcp-server` | `mcp-server` |
| `skill-graph-builder` | `graph-builder` |
| `skill-compiler` | `compiler` |
| `skill-maintenance` | `maintenance` |
| `skill-admin` | `admin` |
| `skill-session-extractor` | `session-extractor` |

This is a readability and API-hygiene improvement, not just cosmetic renaming. It reduces namespace bloat, makes feature-home naming more legible, and avoids coupling every internal package name to one repeated domain word. Apply it consistently across architecture docs, plan slices, crate names, and Rust module namespaces when the canonical V1.1 structure is frozen.

## Top Recommendations by Impact

| Priority | Action | Scope | Expected Benefit |
|---|---|---|---|
| P0 | Canonicalize the architecture and plan into one ownership matrix for crates/modules, tools, events, and state transitions | `docs/architecture`, `docs/plans` | Removes the biggest implementation-drift risk before any code exists |
| P0 | Resolve `extract_session` transport and trust contract | MCP tool schema, Docker topology, hook docs | Closes the end-to-end blocker and eliminates the most dangerous trust boundary |
| P1 | Split `empty_no_match` from `degraded_empty`, and only suppress duplicate injection after a successful compile | `compile_context` contract, session-state semantics, UX policy | Restores user trust and makes failures diagnosable |
| P1 | Pick one V1 scope persistence model and one event catalog, then delete contradictory sections | PG schema, maintenance workflow, V2 readiness claims | Prevents schema churn and lifecycle ambiguity |
| P2 | Publish a concrete 10-minute onboarding guide, capability catalog, and degraded-state runbook | `README`, `CONTRIBUTING`, hook example docs | Improves adoption for the actual target user |

## Suggested Phasing

### Phase A (Immediate)
- Freeze one canonical V1 contract: decomposition, tool list, event list, scope model, transcript transport.
- Add explicit result semantics for `compile_context`: success, no-match, degraded, duplicate-suppressed.

### Phase B (Operational Completion)
- Add stale-context ordering tests across outbox, cache invalidation, rebuild events, and session state.
- Add approval-flow reconciliation for watcher rename misses and failed extractions.

### Phase C (Differentiation)
- Keep the SkillRAE graph, subunit rescue, and maintenance loop as the core differentiators after the trust path is stable.
- Position the system as the semantic runtime layer for AgentSkills-compatible files, not as a new file format.

## Final Assessment

This is a **credible but over-scoped V1**. The idea is stronger than a normal rules-file system and has real differentiation, but the current documents are not ready for implementation because the contracts that matter most for trust and maintainability are still contradictory: ownership, transcript ingestion, state invalidation, and lifecycle/event semantics. Confidence is **medium-high** because the internal evidence is strong and the market comparison is directionally clear. The recommended next step is to **re-scope into a canonical V1.1 doc set** that narrows the initial trust path before any code is written.
