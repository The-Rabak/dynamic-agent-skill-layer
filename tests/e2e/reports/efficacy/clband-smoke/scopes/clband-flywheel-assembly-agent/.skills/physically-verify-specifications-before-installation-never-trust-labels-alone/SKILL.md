---
name: Physically verify specifications before installation — never trust labels alone
description: Before installing any hardware, physically confirm the critical specification (length, diameter, fit, etc.) with direct measurement or observation. Label misprints and mislabeled stock are known failure modes; a ruler or caliper is the ground truth.
tags:
- verification
- hardware
- fasteners
- bearing
- installation
- measurement
type: best_practice
origin: session_extraction
source_session_id: clband-teach-flywheel-assembly-agent
source_provider: claude-code
created_at: 2026-06-12T17:07:02.342240516+00:00
warning_at: 2026-07-12T17:07:02.342240516+00:00
expires_at: 2026-09-10T17:07:02.342240516+00:00
generality: general
generality_rationale: The same 'measure/observe before trusting' discipline appears independently for fastener selection (label error) and bearing installation (technique error), making it a recurring physical-verification principle rather than a one-off rule.
use_when:
- Selecting fasteners from stock for installation
- Installing press-fit or interference-fit components (bearings, bushings, etc.)
- Any hardware installation where a specification mismatch would cause hidden damage or incorrect assembly
avoid_when:
- Parts come pre-kitted and individually verified against the sketch by a dedicated QC step — even then, a spot-check is advisable
artifacts:
- Sketch v2
tools:
- ruler
- caliper
- drift/press tool
invariants:
- The sketch dimension governs; physical measurement confirms compliance.
- No component is installed solely on the basis of its label or assumed identity.
requires:
- Sketch v2 (for reference dimensions)
- Ruler or caliper for fastener measurement
- Appropriate installation tool (drift for bearing press-fit)
produces:
- Confirmed-correct hardware installed with verified specification
- No hidden damage from side-loading or wrong-length fasteners
evidence:
- 'Skill 6: M8x20 fasteners carry a misprinted label — do not use them if the sketch requires 25 mm; verify length with a ruler before use'
- 'Skill 4: For bearing installation, press or tap into the rotor bore using a drift; avoid applying side load'
---

# Physically verify specifications before installation — never trust labels alone

Before installing any hardware, physically confirm the critical specification (length, diameter, fit, etc.) with direct measurement or observation. Label misprints and mislabeled stock are known failure modes; a ruler or caliper is the ground truth.

## Procedures
- For fasteners: measure actual length with a ruler before use, especially when the sketch specifies a precise length that could be confused with a nearby size.
- For press-fit components (bearings, etc.): verify bore fit and use the correct installation tool (drift) to apply axial load only — never side load.
- If a measured specification does not match what the sketch requires, stop and resolve the discrepancy before continuing.

## Conventions
- Treat any label as advisory, not authoritative — the sketch dimension is authoritative.
- Record which physical check was performed and what the measured value was.

