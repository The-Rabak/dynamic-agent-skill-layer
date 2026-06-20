# Lot Movement Plan & Transfer Confirmations
**Prepared:** 2025-07-22 | Reference time: 11:37 (Turn 10 timestamp)  
**Planner:** Material Handler Agent  
**Lots covered:** ABC123-2, HJK890, REG-777

---

## 0. Conflict & Information Resolution Log

Before sequencing any movement, the following conflicts between artifacts and message updates are resolved here. All downstream steps reference these rulings.

| Item | Conflict | Ruling | Authority |
|------|----------|--------|-----------|
| ABC123-2 binning counts | Artifact 2: Good 680 / Rework 520. Artifact 7 (email 11:25): Good 700 / Rework 500 | **Use Artifact 7 corrected counts (Good: 700 / Rework: 500).** Artifact 7 is the later, authoritative email correction from Product Engineering, explicitly superseding v1 of the map. | Artifact 7 (11:25 update) over Artifact 2 |
| HJK890 declared value | Artifact 1: $480,000. Artifact 7 (email 11:20): $650,000 | **Use $650,000 and apply high-value handling protocols.** Finance updated the valuation same-day before movement commenced. | Artifact 7 (11:20 update) over Artifact 1 |
| ABC123-2 scanner (WM-02) | WM-02 near E-Stage-03 is FAILED (Artifact 4, 10:55). Operator_Jen suggests scanning post-move. | **Do not skip pre-move scan. Use WM-03 (Bay C) instead.** Aerospace traceability (Metrology_Tech_Omar, Artifact 1) requires wafer map and die scan at every move — this is non-negotiable. "Scan later" is a protocol violation. | SOP: aerospace traceability; Artifact 4 (WM-03 PASS 11:10) |
| REG-777 cracked container | Quality_Rita flagged hairline crack on container under REG-777. ST-TOTE-09 is also cracked/quarantined (Artifact 3). | **Do not use either cracked container. Assign ST-TOTE-04 (Artifact 3: OK) to REG-777.** HJK890 container gap is flagged as open item below. | Artifact 3; Quality_Rita (Turn 5) |
| Combining lots on one cart | Operator_Jen proposed loading ABC123-2 + HJK890 together, then collecting REG-777. | **Rejected entirely.** (1) REG-777 is under Regulatory Hold — co-loading with non-hold lots is a chain-of-custody violation. (2) ABC123-2 is ESDS — co-loading with non-ESDS lots contaminates ESD protocol integrity. Each lot moves separately with its own chain-of-custody record. | Regulatory Hold SOP; ESD handling SOP |
| Move-first, update-later | Operator_Jen proposed moving first and updating inventory afterward. | **Rejected.** Inventory must be updated in the correct order relative to physical movement (standard SOP). REG-777 specifically requires inventory update *before* SC-2 door access (Artifact 6). | Artifact 6; Facilities_IT (Turn 7): system is online |
| ESD-BGA container availability | ESD-BGA-11 (count 6, exceeds limit) and ESD-BGA-17 (count 5, at limit) both require cleaning before reuse (Artifact 3). | **Both trays require a full cleaning cycle before use.** Dispatch for cleaning immediately. Cleaning cycle: 20 min + 30 min drying + ~10 min travel = ~60 min. Containers available ~12:37. | Artifact 3 |

---

## 1. Open Items Requiring Immediate Resolution (Before or During Execution)

### OPEN-1 — ABC123-2 Dry-Bag Seal Integrity (CRITICAL HOLD RISK)
**Source:** Quality_Rita (Turn 5)  
**Issue:** Nick observed on dry-bag seal at E-Stage-03. ABC123-2 is MSL-3. If the seal is compromised, the lot may have exceeded allowable floor life exposure.  
**Required action before movement:**
1. Perform visual seal integrity check immediately upon reaching E-Stage-03.
2. If seal is intact (nick is cosmetic/on outer packaging, not on the moisture-barrier layer): document finding, proceed with move after re-bagging with fresh desiccant per MSL-3 SOP.
3. If seal is confirmed breached: **HALT movement of ABC123-2.** Initiate MSL-3 bake-out protocol per IPC/JEDEC J-STD-033. Bake-out duration (typically 48–168 hours depending on conditions and device body size) must be confirmed with Product Engineering and Lab Supervisor Elle before RL-2 delivery time is revised. Notify all stakeholders.
4. Update move instructions to include bake-out destination if seal is breached.

**This plan proceeds assuming seal is intact and passes inspection. If seal is breached, ABC123-2 movement is halted pending bake-out completion.**

### OPEN-2 — HJK890 Container Shortage
**Source:** Artifact 3 conflict analysis  
**Issue:** ST-TOTE-04 is the only confirmed available clean standard tote. It must be assigned to REG-777 (time-critical). ST-TOTE-09 is quarantined. No second confirmed clean standard tote is listed.  
**Required action:**
- Immediately contact Receiving/Warehouse to source a second clean standard tote suitable for high-value ($650K) shipment to SP-5.
- Verify whether SP-5 requires specific high-value shipping containers beyond a standard tote (padded, tamper-evident, etc.) — Finance_Luc and Shipping Prep should confirm.
- **HJK890 movement is gated on securing an appropriate container. Do not move HJK890 until a confirmed clean, high-value-compliant container is available.**

### OPEN-3 — UV Transfer Chamber Cycle Time
**Source:** Orchestrator (Turn 10)  
**Issue:** UV transfer chamber is required for ABC123-2 (Class 10,000 → Class 100 transition). Cycle time is not stated in any artifact.  
**Required action:** Confirm UV transfer chamber availability and cycle time with Facilities before ABC123-2 reaches the chamber. Build cycle time into final timing estimate. This plan uses a placeholder of [UV-CYCLE] in the sequence below.

### OPEN-4 — ION-04 Re-Verification Before RL-2 Intake
**Source:** Artifact 4 (last verified 08:10 — ~3.5 hours before current time); Lab_Supervisor_Elle (Turn 9)  
**Issue:** ION-04 at RL-2 passed at 08:10 but has not been re-checked closer to the transfer window.  
**Required action:** Request RL-2 ionizer re-verification from Lab_Supervisor_Elle or her technician before ABC123-2 arrives. If ION-04 fails re-check, lot cannot be received into RL-2 until a compliant ionizer is operational.

---

## 2. Route Conditions

**Peak restriction window:** 11:30–13:00 (currently active at 11:37)  
**Alternate service route:** approximately 3× standard transit time. All movements within 11:37–13:00 use alternate service route. Movements scheduled after 13:00 may use main corridor.  

---

## 3. Movement Sequence by Priority

### Priority 1 — REG-777 (Regulatory Hold Deadline: 12:15)

**Time budget from 11:37:** 38 minutes to deadline.

| Step | Action | Responsible | Est. Time |
|------|--------|-------------|-----------|
| 1 | Retrieve ST-TOTE-04 from near E-Stage-03 (do NOT touch ESD containers — this is a standard tote for non-ESDS lot). | Material Handler | 11:37 |
| 2 | At T-09 staging cart: visually confirm REG-777 lot ID and unit count. Transfer lot from cracked container into ST-TOTE-04. Seal and label ST-TOTE-04 with REG-777 lot ID, hold status, and handler ID. Quarantine the cracked source container. | Material Handler | 11:38–11:42 |
| 3 | Update inventory system: record container swap for REG-777 (cracked container → ST-TOTE-04). Process as individual transactions (not bulk) given 2–3s system delay. | Material Handler | 11:42–11:45 |
| 4 | Complete inventory update: record REG-777 source location departure (T-09 staging cart). **This must complete before SC-2 door access request per Artifact 6.** | Material Handler | 11:45–11:48 |
| 5 | Contact Security_Ana for escort. REG-777 requires Material Handler + Security access. Do not proceed to SC-2 without escort. | Material Handler (call Security_Ana) | 11:48 |
| 6 | Move REG-777 (in ST-TOTE-04) via alternate service route to SC-2, with Security_Ana escort. Note: ION-02 at T-09 is FAILED — this does not block movement as REG-777 is not classified ESDS, but note for T-09 area status. | Material Handler + Security_Ana | 11:50–12:00 (est., alternate route) |
| 7 | At SC-2: submit access request (inventory update already complete per Step 4). Security_Ana verifies access. Intake REG-777 into secure cage per SC-2 intake checklist. | Material Handler + Security_Ana | 12:00–12:10 |
| 8 | Confirm secure cage door locked, SC-2 intake checklist signed. Update inventory: record SC-2 destination arrival. | Material Handler | 12:10–12:12 |

**Deadline compliance:** Lot secured by ~12:10–12:12, within the 12:15 hard deadline. ✓

---

### Priority 2 — ABC123-2 (Aerospace Traceability + MSL-3 + ESDS)

**Gate dependency:** Seal integrity check (OPEN-1) must pass before proceeding. Container cleaning running concurrently.

#### 2a. Concurrent preparation tasks (begin at 11:37 while REG-777 prep is being coordinated)

| Step | Action | Responsible | Est. Time |
|------|--------|-------------|-----------|
| C1 | Dispatch ESD-BGA-11 and ESD-BGA-17 to Building B cleaning station. Log them out of service. Note expected return: ~12:37 (5 min travel + 20 min clean + 30 min dry + 5 min return). Confirm with Building B that both trays are accepted into cleaning queue simultaneously. | Material Handler / designee | 11:37 |
| C2 | Request ION-04 re-verification at RL-2 from Lab_Supervisor_Elle (OPEN-4). | Material Handler (message Elle) | 11:38 |
| C3 | Confirm UV transfer chamber availability and cycle time (OPEN-3). | Material Handler (message Facilities) | 11:38 |

#### 2b. Main ABC123-2 sequence

| Step | Action | Responsible | Est. Time |
|------|--------|-------------|-----------|
| 1 | At E-Stage-03: locate and use the grounding verification station. Perform ESD wrist strap check. Do not touch ABC123-2 or any ESDS material until strap is verified compliant. | Material Handler | ~11:50 |
| 2 | Perform dry-bag seal integrity check on ABC123-2 (per OPEN-1). **If breached: halt, initiate bake-out, notify stakeholders. Do not proceed.** If intact: document, re-bag with fresh desiccant per MSL-3 SOP, and proceed. | Material Handler + Quality_Rita confirmation | ~11:52 |
| 3 | Move ABC123-2 lot (in temporary ESD-safe staging) to WM-03, Bay C, via alternate service route. WM-02 is FAILED (Artifact 4) — WM-03 is the only compliant alternative. Queue is ~15 min. | Material Handler (ESD precautions maintained throughout) | ~11:55 |
| 4 | Queue and complete wafer map and die scan at WM-03. **Scanning must complete before lot moves to RL-2 — aerospace traceability requires scan at every move. "Scan at destination" is not compliant.** Record scan outputs to AT-7 program traceability file. | Material Handler + WM-03 operator | ~12:10–12:30 |
| 5 | Collect cleaned and dried ESD-BGA-11 and ESD-BGA-17 from Building B. Verify cleaning log sign-off for each tray. | Material Handler / designee | ~12:37 |
| 6 | Load segregated containers (ESD gloves, wrist strap verified): **ESD-BGA-11 → 700 Good units** (Artifact 7 corrected count). **ESD-BGA-17 → 500 Rework units** (Artifact 7 corrected count). Seal and label each container with lot ID, bin category, unit count, date, and handler ID. Do not co-mingle. Total: 1,200 units accounted for (0 Scrap per both Artifact 2 and Artifact 7). | Material Handler | ~12:37–12:50 |
| 7 | Update inventory: record container assignment and bin split for ABC123-2 (Good: 700 in ESD-BGA-11; Rework: 500 in ESD-BGA-17). Process as individual transactions. | Material Handler | ~12:50–12:53 |
| 8 | Confirm ION-04 re-verification result from RL-2 (OPEN-4). If ION-04 is FAILED: halt delivery until resolved. If PASS: proceed. | Material Handler | ~12:53 |
| 9 | Move ABC123-2 (both containers) through UV transfer chamber (Class 10,000 → Class 100 transition). Complete UV cycle [UV-CYCLE duration TBD per OPEN-3]. Maintain ESD precautions throughout. | Material Handler | ~12:55 + [UV-CYCLE] |
| 10 | After 13:00, main corridor is available. Move to Rework Lab RL-2 via cleared main corridor (or continue on alternate route if UV cycle completes before 13:00). | Material Handler | ~13:00+ |
| 11 | Deliver to RL-2. Confirm ION-04 is active. Hand off both ESD-BGA containers to RL-2 receiving technician. Obtain signed receipt. | Material Handler + RL-2 technician | ~13:05–13:10 |
| 12 | Update inventory: record ABC123-2 arrival at RL-2 with container IDs, bin counts, and scan reference. Process as individual transactions. | Material Handler | ~13:10–13:15 |

---

### Priority 3 — HJK890 (High-Value, Non-ESDS)

**Gate dependency:** Confirmed clean, high-value-compliant container required (OPEN-2). Movement cannot begin until container is secured.

**Assumed available from:** ~12:10 (after REG-777 is secured and Material Handler is free), subject to container availability.

| Step | Action | Responsible | Est. Time |
|------|--------|-------------|-----------|
| 1 | Confirm replacement container secured (OPEN-2). Verify container is appropriate for $650,000 declared value per high-value handling protocol. ST-TOTE-09 is quarantined — do not use. ST-TOTE-04 is assigned to REG-777 and will remain at SC-2 with that lot. | Material Handler / Warehouse | ASAP after 12:00 |
| 2 | Apply high-value handling protocols per facility SOP (e.g., dual-handler verification, tamper-evident sealing, documentation of handler IDs). Finance_Luc to confirm specific high-value protocol requirements for $650K threshold. | Material Handler + Finance_Luc | At container pickup |
| 3 | At QA Vault QV-1: verify HJK890 lot ID, unit count, and current container condition. Load into confirmed clean high-value container. Seal, label, and document. | Material Handler | ~12:10–12:20 (est., subject to OPEN-2) |
| 4 | Update inventory: record QV-1 departure for HJK890. Process as individual transaction. | Material Handler | ~12:20–12:22 |
| 5 | Move HJK890 to Shipping Prep SP-5. If before 13:00: use alternate service route. If after 13:00: main corridor available. | Material Handler | ~12:22–12:35 (est., alternate route if pre-13:00) |
| 6 | Deliver to SP-5. Confirm receipt with SP-5 receiving staff. Obtain signed receipt referencing updated declared value ($650,000). | Material Handler + SP-5 staff | ~12:35–12:40 |
| 7 | Update inventory: record SP-5 arrival for HJK890. | Material Handler | ~12:40–12:42 |

---

## 4. Explicit SOP Violations Rejected

The following actions proposed in the conversation transcript are **not approved** and must not be executed:

| Proposed by | Proposal | Reason for rejection |
|-------------|----------|----------------------|
| Operator_Jen (Turn 2) | Load ABC123-2 and HJK890 together on one cart | Co-loading ESDS and non-ESDS lots violates ESD chain-of-custody protocol. Also non-hold and hold lots must not be co-transported. |
| Operator_Jen (Turn 2) | "Move first and update [inventory] later" | Standard SOP requires inventory updates in the correct sequence relative to physical movement. REG-777 specifically requires inventory update before SC-2 door access (Artifact 6). |
| Operator_Jen (Turn 12) | Push ABC123-2 to RL-2 and "scan later at their station" | Aerospace traceability requires wafer map and die scanning at every move, not at destination. Scan must complete at WM-03 before movement to RL-2. |

---

## 5. Lot Transfer Confirmation Drafts

---

### LTC-001 — REG-777

```
LOT TRANSFER CONFIRMATION
─────────────────────────────────────────────────────────────────
Lot ID:           REG-777
From:             T-09 Staging Cart
To:               Secure Cage SC-2
Program:          [not specified in artifacts — confirm with originating program owner]

Container:
  Source container:    [cracked, unassigned ID] — QUARANTINED, removed from service
  Transfer container:  ST-TOTE-04 (verified clean, Artifact 3)

Timestamps (estimated):
  Container swap / lot transfer into ST-TOTE-04:   2025-07-22 ~11:38–11:42
  Inventory update (departure, T-09):              2025-07-22 ~11:45–11:48
  Departed T-09 (with Security escort):            2025-07-22 ~11:50
  Arrived SC-2 (secured in cage):                  2025-07-22 ~12:10 (est.)
  Inventory update (arrival, SC-2):                2025-07-22 ~12:10–12:12

Special Handling Notes:
  - Regulatory Hold issued 10:15. SC-2 deadline: 12:15. Target arrival: ~12:10.
  - Material Handler + Security escort required (Security_Ana assigned).
  - Inventory update MUST complete before SC-2 door access request (Artifact 6).
  - Alternate service route mandatory (peak restriction 11:30–13:00).
  - Cracked source container quarantined; do not return to service without inspection.
  - ION-02 at T-09 is FAILED — noted for maintenance awareness; no ESD classification
    on this lot, movement not blocked.
  - All inventory transactions processed individually (no bulk) per Facilities_IT advisory.

Handler:          [Material Handler ID]
Security Escort:  Security_Ana
Authorization:    Regulatory Hold Notice (Artifact 6)
─────────────────────────────────────────────────────────────────
```

---

### LTC-002 — ABC123-2

```
LOT TRANSFER CONFIRMATION
─────────────────────────────────────────────────────────────────
Lot ID:           ABC123-2 (child lot of ABC123)
From:             E-Stage-03 (Class 10,000 cleanroom)
To:               Rework Lab RL-2 (Class 100 cleanroom)
Program:          AT-7 | MSL: 3 | Classification: ESDS

Containers:
  ESD-BGA-11 (post-cleaning, Building B):  700 Good units
  ESD-BGA-17 (post-cleaning, Building B):  500 Rework units
  Total:                                   1,200 units (0 Scrap)
  Binning authority: Artifact 7 correction (11:25 email, Product Engineering)
                     — supersedes Artifact 2 v1 counts

Timestamps (estimated):
  ESD wrist strap verification:            2025-07-22 ~11:50
  Dry-bag seal integrity check:            2025-07-22 ~11:52
  [CONDITIONAL: If seal breached — HALT. Bake-out required. ETA TBD.]
  ESD containers dispatched for cleaning:  2025-07-22 ~11:37 (concurrent)
  Lot moved to WM-03 for scan:             2025-07-22 ~11:55
  Wafer map / die scan completed (WM-03):  2025-07-22 ~12:10–12:30 (est., ~15 min queue)
  Cleaned containers retrieved:            2025-07-22 ~12:37
  Segregated container loading:            2025-07-22 ~12:37–12:50
  Inventory update (container/bin split):  2025-07-22 ~12:50–12:53
  ION-04 (RL-2) re-verification confirmed: 2025-07-22 ~12:53
  UV transfer chamber cycle (10K→100):     2025-07-22 ~12:55 + [UV-CYCLE TBD]
  Departed for RL-2:                       2025-07-22 ~13:00+
  Arrived RL-2:                            2025-07-22 ~13:05–13:10 (est.)
  Inventory update (RL-2 arrival):         2025-07-22 ~13:10–13:15

Special Handling Notes:
  - ESDS: ESD wrist strap check mandatory at E-Stage-03 grounding station before any touch.
  - MSL-3: Dry-bag seal must be inspected before movement. If breached: halt, bake-out SOP.
  - Aerospace traceability (AT-7): wafer map and die scan required at this move. Scanner
    WM-02 is FAILED — WM-03 (Bay C) used. Scan record must reference AT-7 program file.
    "Scan at destination" is NOT compliant; scan must precede movement.
  - Bin segregation MANDATORY: Good (700) and Rework (500) in separate sealed containers.
    No co-mingling. Scrap: 0 — verify counts at loading.
  - Both ESD-BGA trays required cleaning cycle (Artifact 3). Confirm cleaning log
    sign-off before loading.
  - UV transfer chamber required for Class 10,000 → Class 100 transition.
    Cycle time must be confirmed with Facilities (OPEN-3).
  - ION-04 re-verification required before RL-2 receipt (last verified 08:10). If FAILED,
    hold lot until ionizer is repaired or a compliant replacement is confirmed.
  - Alternate service route used for all movement during peak window (11:30–13:00).
    Main corridor available after 13:00.
  - All inventory transactions processed individually per Facilities_IT advisory.

Handler:          [Material Handler ID]
ESD Verification: [Grounding station ID near E-Stage-03]
Scan Station:     WM-03, Bay C
Authorization:    Artifact 1 (move instruction) + AT-7 aerospace traceability SOP
─────────────────────────────────────────────────────────────────
```

---

### LTC-003 — HJK890

```
LOT TRANSFER CONFIRMATION
─────────────────────────────────────────────────────────────────
Lot ID:           HJK890
From:             QA Vault QV-1
To:               Shipping Prep SP-5
Program:          FT-2 | Classification: Non-ESDS

Container:
  ST-TOTE-04: UNAVAILABLE — assigned to REG-777 (Lot LTC-001)
  ST-TOTE-09: QUARANTINED — cracked side (Artifact 3), do not use
  REQUIRED: Second confirmed clean standard tote / high-value shipping container
             [OPEN-2 — MOVEMENT BLOCKED UNTIL RESOLVED]

Declared Value:   $650,000
Value authority:  Artifact 7 (Finance email 11:20) — supersedes Artifact 1 ($480,000)
Handling tier:    HIGH-VALUE PROTOCOL REQUIRED

Timestamps (estimated — pending OPEN-2 container resolution):
  Container confirmed and received:         TBD (OPEN-2)
  Inventory update (departure, QV-1):       TBD
  Departed QV-1:                            TBD (~12:10–12:20 if container secured)
  Arrived SP-5:                             TBD (~12:35–12:40, alternate route if pre-13:00)
  Inventory update (arrival, SP-5):         TBD

Special Handling Notes:
  - HIGH-VALUE ($650,000): apply facility high-value handling protocols.
    Confirm dual-handler verification, tamper-evident sealing, and documentation
    requirements with Finance_Luc before movement begins.
  - Movement BLOCKED until clean, high-value-compliant container is sourced (OPEN-2).
    Contact Warehouse/Receiving immediately.
  - Alternate service route if moving before 13:00. Main corridor available after 13:00.
  - SP-5 receiving staff must acknowledge declared value of $650,000 on signed receipt.
  - All inventory transactions processed individually per Facilities_IT advisory.
  - No ESD precautions required (Non-ESDS classification).

Handler:          [Material Handler ID]
Authorization:    Artifact 1 (move instruction) + Artifact 7 (value update)
─────────────────────────────────────────────────────────────────
```

---

## 6. Summary of Open Items and Required Escalations

| ID | Issue | Gating | Owner to contact | Urgency |
|----|-------|--------|------------------|---------|
| OPEN-1 | ABC123-2 dry-bag seal integrity — inspect before movement | ABC123-2 movement | Quality_Rita, Product_Engineer_Mark | Immediate |
| OPEN-2 | HJK890 container shortage — second clean standard/high-value tote needed | HJK890 movement | Warehouse/Receiving, Finance_Luc | High |
| OPEN-3 | UV transfer chamber cycle time for ABC123-2 | ABC123-2 schedule | Facilities | Before ~12:50 |
| OPEN-4 | ION-04 re-verification at RL-2 | ABC123-2 RL-2 delivery | Lab_Supervisor_Elle | Before ~12:53 |
| — | Bake-out protocol details if OPEN-1 confirms seal breach | ABC123-2 rework timeline | Product_Engineer_Mark, Lab_Supervisor_Elle | Conditional |

---

## 7. Artifact / Message Items Explicitly Excluded from Plan

| Source | Item | Reason for exclusion |
|--------|------|----------------------|
| Artifact 8 | Cafeteria specials | Irrelevant to lot movement — no action. |
| Operator_Jen (Turn 12) | Lunch plans | Personal — no action. |
| Operator_Jen (Turn 2) | Combined cart / move-first approach | Protocol violations — rejected (Section 4). |
