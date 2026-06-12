---
name: Always trace every decision to a named authoritative artifact
description: Every specification, part selection, or procedural choice must be anchored to a specifically named, official artifact (sketch revision, batch ID, manifest, etc.). Improvising from memory, using unlabeled defaults, or citing secondary sources is not acceptable.
tags:
- traceability
- authority
- documentation
- parts
- specifications
type: principle
origin: session_extraction
source_session_id: clband-teach-flywheel-assembly-agent
source_provider: claude-code
created_at: 2026-06-12T17:07:02.341211795+00:00
warning_at: 2026-07-12T17:07:02.341211795+00:00
expires_at: 2026-09-10T17:07:02.341211795+00:00
generality: general
generality_rationale: Recurs across four distinct decision types (torque, parts sourcing, document policy, batch selection), making it a session-wide discipline rather than a task-specific rule.
use_when:
- Selecting a part or fastener
- Choosing a torque value or installation procedure
- Sourcing any component from inventory
- Any decision that must be auditable or reproducible
avoid_when:
- The artifact has not been provided or verified — raise the gap instead of guessing
- A later revision of the artifact exists but has not been officially adopted
artifacts:
- Sketch v2
- Batch FW-2025-0118
- Artifact 3 (Warehouse Parts Bin Manifest, Agent E)
invariants:
- The governing artifact must be named explicitly, not implied.
- Only officially provided documents are valid sources; do not substitute secondary or derived references.
requires:
- Official sketch (at a specific named revision)
- Batch ID for consumables/parts
- Named manifest or parts-source artifact
produces:
- Fully traceable decision record tied to a specific artifact version
evidence:
- 'Skill 3: Use the torque callout specified in Sketch v2'
- 'Skill 5: Reference Artifact 3 (Warehouse Parts Bin Manifest, Agent E) as the relevant parts source'
- 'Skill 7: Use only the official sketch and documents provided'
- 'Skill 1: Use batch FW-2025-0118'
---

# Always trace every decision to a named authoritative artifact

Every specification, part selection, or procedural choice must be anchored to a specifically named, official artifact (sketch revision, batch ID, manifest, etc.). Improvising from memory, using unlabeled defaults, or citing secondary sources is not acceptable.

## Procedures
- Identify the authoritative artifact that governs the decision at hand (sketch revision, batch number, manifest agent, etc.).
- Cite that artifact explicitly when recording or communicating the decision.
- If no authoritative artifact exists for a decision, surface the gap — do not fill it with an assumption or a default.

## Conventions
- Refer to sketch by revision (e.g., 'Sketch v2'), not generically.
- Refer to parts source by artifact name and agent (e.g., 'Artifact 3, Agent E').
- Refer to batch by exact ID (e.g., 'FW-2025-0118').

