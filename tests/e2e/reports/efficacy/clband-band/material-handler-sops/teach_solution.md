# Execution Plan and Lot Transfer Confirmation
**Move Order:** LMI-2025-11-20-001
**Lot:** LOT-AX57 | QFP-128, ESDS, MSL3, Aerospace
**Source:** Warehouse A (Class 10,000)
**Destination:** Tester T-12 Intake Rack, Cleanroom 3 (Class 100)
**Plan Generated:** ~11:05, 2025-11-20

---

## Section 0 — Pre-Execution Blockers and Assessments

Three hard blockers and two rejections must be logged before any physical movement begins.

### Blocker 1 — MSL3 Moisture Barrier Bag Seal Breach (CRITICAL — Immediate Action)

Operator T-12 (message 9, 10:57) reported opening one moisture barrier bag seal "earlier this morning" to sample a tube, then re-taping it. Per SOP Cold Chain Rule 3, any broken MSL bag seal requires the affected material to be returned to the bake-out oven within 15 minutes, regardless of scheduled destination. The 15-minute window has already elapsed. The affected material cannot be staged at T-12 today.

**Required action:** Coordinate immediately with Operator T-12 to identify the specific moisture barrier bag (MBB) that was opened and all tube IDs inside it. All tubes that were inside that MBB are now humidity-exposed and must be quarantined for bake-out per IPC/JEDEC J-STD-033. MSL3 bake-out at 125 °C runs approximately 24 hours. The affected units will not be available for staging until bake-out completes. Manufacturing Manager and Quality must be notified now.

Re-taping is not an acceptable substitute for an intact factory seal. Operator T-12's assessment that "the parts inside should be fine" does not override this SOP requirement.

### Blocker 2 — Insufficient ESDS Containers

Artifact 3 documents:
- 12 ESDS QFP tubes: use count ≤ 5, clean, ready to use
- 7 ESDS QFP tubes: use count = 5, require cleaning before next use (Building B, 20 min clean + 30 min dry = ~50 min)
- 1 ESDS QFP tube: damaged (hairline crack) — quarantine, do not use under any circumstances
- 22 non-ESD QFP tubes: not permissible for ESDS material

Total ESDS tubes available after cleaning: 12 + 7 = 19
Tubes required for this lot: 24 (Good) + 3 (Rework) + 1 (Scrap) = 28
Shortfall: 9 ESDS tubes

Allocation plan from 19 available tubes:
- Rework bin: 3 tubes (from clean set)
- Scrap bin: 1 tube (from clean set)
- Good bin, Batch 1: 8 tubes (from remaining clean set) → 400 units
- Good bin, Batch 2: 7 tubes (from cleaned set, available ~12:00) → 350 units
- Good bin, remaining: 450 units — cannot be packaged. ESDS container shortage; non-ESD tubes are prohibited.

The 450 unpackaged Good units must remain at Warehouse A with a HOLD — AWAITING ESDS CONTAINERS tag in IMS. Manufacturing Manager must be notified and must source 9 additional ESDS QFP tubes before those units can move.

### Blocker 3 — High-Value Lot: Continuous Escort Is Mandatory

LMI Artifact 1 states lot value at $520,000. Finance Artifact 7 states $480,000, but that figure is explicitly from "last week" and Artifact 7 itself confirms the LMI value today is $520,000. The LMI is the authoritative document for this move. Finance's prior-week estimate does not modify the LMI value or override the escort requirement.

$520,000 > $500,000 threshold (SEC-HV-01). Continuous visual escort by a Security Officer or trained handler buddy is mandatory throughout all transport and staging. The lot cannot leave Warehouse A until escort is confirmed and the escort officer is physically present.

### Rejected Instructions

**Operator T-12 (message 3) — co-transport LOT-BQ90 on the same cart:**
Rejected. SOP Rule 1 (Single Lot Transport) permits only one lot per cart at a time. LOT-BQ90 cannot share a cart with LOT-AX57 under any circumstances, including time pressure.

**Operator T-12 (message 3) — cut through main corridor:**
Rejected pending route assessment. The main corridor restriction runs 11:30–13:00. Given the pre-move steps required, departure cannot occur before approximately 11:25. Any transit that reaches the main corridor at or after 11:30 must use the alternate route (3× standard travel time). Alternate route is planned for all trips.

**Operator T-12 (message 9) — treat re-taped bag as intact, keep moving:**
Rejected. Cold Chain Rule 3 is explicit: a broken seal requires bake-out regardless of apparent condition. Re-taping does not constitute an intact moisture barrier.

**Manufacturing Manager (message 10) — skip or abbreviate steps to meet 11:40 schedule:**
Rejected. ESD wrist strap verification, IMS pre-move confirmation, UV transfer chamber cycle, and ionizer check are non-waivable SOP requirements. The 11:40–11:45 staging deadline cannot be met while remaining compliant. Manufacturing Manager has been notified of the revised timeline (see Section 4).

---

## Section 1 — Parallel Immediate Actions (~11:05–11:12)

These actions run concurrently before packing begins. They do not depend on each other.

### Action A — ESD Wrist Strap Verification

1. Walk immediately to the nearest ESD grounding verification station (one of 8 facility-wide).
2. Test wrist strap resistance. Required result: < 1 megaohm.
3. Record in ESD log: resistance reading, station ID, timestamp, pass/fail, operator ID.
4. **If FAIL:** proceed to supply cage for a replacement wrist strap (10–15 min), then retest at the grounding station before touching any device. Do not touch any ESDS material with an unverified or failed strap.
5. Do not handle LOT-AX57 or any ESDS material until this step is documented PASS.

### Action B — MSL3 Bake-Out Response (Urgent)

1. Notify Quality by radio or phone of the MSL3 seal breach on LOT-AX57.
2. Contact Operator T-12 to identify: (a) which MBB was opened, (b) all tube IDs inside that MBB, and (c) approximate time of opening.
3. Attach a yellow BAKE-OUT REQUIRED / DO NOT STAGE tag to each affected tube.
4. Log in IMS: MBB ID, affected tube IDs, approximate breach time, bake-out start time, destination (bake-out oven area).
5. Move affected tubes to the bake-out oven area. This is a separate physical move from the main staging operation and must also comply with ESD and lot tracking requirements.
6. Inspect all remaining sealed MBBs for LOT-AX57: verify intact seals and green humidity indicator cards. Any bag with a compromised seal or red/yellow humidity indicator must also be quarantined for bake-out.
7. Notify Manufacturing Manager: affected units are not available for T-12 staging today. Bake-out completion is approximately 24 hours from start.

### Action C — Quarantine Damaged Tube

1. Identify the 1 cracked QFP tube from the container inventory (Artifact 3).
2. Attach QUARANTINE — DO NOT USE tag.
3. Log in IMS: tube ID, defect description (hairline crack identified at incoming inspection), action (quarantine pending Quality disposition), date/time.
4. Notify Quality per SOP: damaged containers must be quarantined and reported. Do not use this tube even if visually "mostly OK."
5. Assign container ID: ESDS-QFP-AX57-DMG001. Set aside; do not include in any lot packaging.

### Action D — Initiate Tube Cleaning

1. Identify the 7 ESDS QFP tubes at use count = 5 (require cleaning before next use).
2. Transport them to Building B cleaning station immediately. Begin cleaning no later than 11:10.
3. Log: tube IDs, cleaning start time (~11:10), expected cleaning completion (~11:30), expected drying completion (~12:00).
4. These 7 tubes are not available until drying is complete. Do not use wet tubes.

### Action E — Confirm High-Value Escort

1. Contact Security immediately to assign a Security Officer or arrange a trained handler buddy.
2. Escort must be present before LOT-AX57 leaves Warehouse A and must maintain continuous visual monitoring through all transit, transfer chamber passage, and T-12 staging.
3. Lot cannot be left unattended at any point. This applies even at brief stops and during the UV cycle wait.
4. Log escort officer name/ID and confirmed start time.

### Action F — Route and Scanner Confirmation

1. Aisle B auxiliary scanner is DOWN 10:30–13:00. Do not use. All wafer map and die ID scanning will be performed at the Cleanroom 3 scanner near T-12, which is confirmed operational.
2. Main corridor restriction: 11:30–13:00. Given pre-move steps, transport departure will occur at approximately 11:25. Use the alternate route for all four transport trips.
3. Notify Manufacturing Manager that alternate route is in use and that each trip will take approximately 3× standard corridor travel time.

---

## Section 2 — Sequential Move Execution

### Step 1 — Pre-Move IMS Update (~11:10–11:12)

Must be completed before any physical movement of LOT-AX57 material (SOP Rule 10).

1. Open IMS. Submit pre-move location transaction:
   - Lot ID: LOT-AX57
   - From: Warehouse A, [source shelf ID]
   - To: Tester T-12 Intake Rack, Cleanroom 3
2. Wait for system confirmation. IT advisory (Artifact 6) warns of 30–90 second commit times.
3. **If timeout:** retry once. If second timeout or outage, hold all physical movement and notify Manufacturing Manager. Do not proceed until IMS confirms.
4. Record confirmed transaction ID. This confirmation is the prerequisite for any physical movement.

### Step 2 — Pack Good Bin, Batch 1 (~11:12–11:22)

Pre-conditions: ESD wrist strap verified PASS (Action A), escort confirmed (Action E), IMS confirmed (Step 1).

Container allocation from 12 clean ESDS tubes: reserve 3 tubes for Rework (R001–R003) and 1 tube for Scrap (S001). The remaining 8 clean tubes go to Good Batch 1.

Assigned IDs: ESDS-QFP-AX57-G001 through ESDS-QFP-AX57-G008

1. At ESD-safe workstation (wrist strap grounded, ESD mat in use), load 50 Good-bin units per tube into G001–G008 (400 units total). Exclude any Good bin units identified in Action B as being inside the opened MBB — route those to bake-out.
2. Apply GREEN label to each tube:
   `LOT-AX57 / BIN: GOOD / ESDS / MSL3-SEALED / 50 units / [Tube ID] / LMI-2025-11-20-001`
3. Seal each tube per ESDS protocol.
4. Wafer map and die ID scanning will be performed at the Cleanroom 3 scanner upon arrival at T-12 (scanner is operational there; Aisle B scanner is down).

Weight check: 8 tubes × ~1.83 lb = ~14.7 lb. Cart limit: 50 lb. PASS.

### Step 3 — Pack Rework Bin (~11:12–11:22, concurrent with Step 2)

Assigned IDs: ESDS-QFP-AX57-R001, ESDS-QFP-AX57-R002, ESDS-QFP-AX57-R003

1. Load 50 Rework-bin units per tube (150 units total) into R001–R003.
2. Apply YELLOW label:
   `LOT-AX57 / BIN: REWORK / ESDS / MSL3-SEALED / 50 units / [Tube ID] / LMI-2025-11-20-001`
3. Seal each tube.

Weight check: ~6.5 lb. PASS.

### Step 4 — Pack Scrap Bin (~11:12–11:22, concurrent)

Assigned ID: ESDS-QFP-AX57-S001

1. Load 30 Scrap-bin units into S001.
2. Apply RED label:
   `LOT-AX57 / BIN: SCRAP / ESDS / MSL3-SEALED / 30 units / S001 / LMI-2025-11-20-001`
3. Seal tube.

Weight check: ~0.8 lb. PASS.

**Future note:** When Scrap bin is eventually moved to the scrap disposal area (not this staging move), SOP Rule 8 requires secondary verification from a Quality Inspector before physical movement. That verification takes 15–30 minutes and cannot be bypassed. Flag this for the operator managing final scrap disposition.

---

### Transport Trip 1 — Good Bin Batch 1 (~11:25 departure)

Bins travel on separate cart trips (SOP Rule 2: cannot transport mixed bin categories on the same cart, even with dividers). Good Batch 1 goes first.

**Pre-departure checklist:**
- [ ] ESD wrist strap verified PASS (re-verify if >2 hours since last check at departure time)
- [ ] Escort officer present and confirmed
- [ ] IMS transaction confirmed (Step 1)
- [ ] Cart loaded with G001–G008 only (14.7 lb, no other lots, no other bin categories)
- [ ] Alternate route selected (main corridor restricted 11:30–13:00)

**Transit:**
1. Load tubes G001–G008 onto cart.
2. Depart Warehouse A via alternate route. Escort maintains continuous visual contact.
3. Proceed to Class 10,000 → Class 100 transfer chamber.
4. Pass material through transfer chamber; initiate 10-minute UV sanitization cycle (SOP Rule 6). Escort and handler wait with the lot. Do not leave it unattended during the UV cycle.
5. After UV cycle completes, proceed into Cleanroom 3 to Tester T-12.

**At T-12:**
1. Before placing any material, perform 2-minute ionizer check at T-12 station (SOP ESD requirement; the 09:15 Facilities verification is not a substitute for the pre-placement check). Log result and time.
2. If ionizer check FAILS, do not place material on station. Notify Facilities and hold at intake rack until resolved.
3. Scan wafer map and die IDs for all units in G001–G008 at the Cleanroom 3 scanner near T-12. This is the only operational scanner during this window (Aisle B is down). Estimated time: 5–10 minutes.
4. Stage tubes G001–G008 at the T-12 sample intake rack per LMI instruction. Do not place on the tester station itself until the ionizer check is complete and operator confirmation is received.
5. Physically segregate from Rework and Scrap staging positions (separate labeled section of rack).
6. Update IMS: G001–G008 located at T-12 Intake Rack, Cleanroom 3. Log timestamp and IMS confirmation.

Estimated Good Batch 1 staged: approximately 12:05.

---

### Transport Trip 2 — Rework Bin (~12:20 departure, after returning from Trip 1)

**Pre-departure checklist:**
- [ ] Good Batch 1 confirmed in IMS at T-12
- [ ] ESD wrist strap re-verified
- [ ] Escort present
- [ ] IMS sub-update: log Rework bin departure from Warehouse A
- [ ] Cart loaded with R001–R003 only (~6.5 lb)
- [ ] Alternate route

**Transit and T-12 arrival:** Follow same procedure as Trip 1.
- 10-minute UV cycle at transfer chamber
- 2-minute ionizer recheck at T-12 before placing
- Wafer map and die ID scan for Rework units at Cleanroom 3 scanner
- Stage R001–R003 at T-12 intake rack, physically segregated from Good bin section (YELLOW label section)
- Update IMS: R001–R003 at T-12 Intake Rack, Cleanroom 3

Estimated Rework staged: approximately 13:05.

---

### Transport Trip 3 — Scrap Bin (~13:20 departure)

**Pre-departure checklist:**
- [ ] Rework confirmed in IMS at T-12
- [ ] ESD wrist strap re-verified
- [ ] Escort present
- [ ] IMS sub-update: log Scrap bin departure from Warehouse A
- [ ] Cart loaded with S001 only (~0.8 lb)
- [ ] Alternate route

**Transit and T-12 arrival:** Follow same procedure as Trips 1 and 2.
- 10-minute UV cycle
- 2-minute ionizer recheck
- Wafer map and die ID scan for Scrap units
- Stage S001 at T-12 intake rack, physically segregated from Good and Rework sections (RED label section)
- Update IMS: S001 at T-12 Intake Rack, Cleanroom 3

Estimated Scrap staged: approximately 14:05.

**Reminder:** Eventual movement of S001 to scrap disposal requires Quality Inspector dual-verification (SOP Rule 8, 15–30 min). Do not move to disposal without it.

---

### Step 5 — Pack Good Bin, Batch 2 (~12:00 when cleaned tubes return from Building B)

Cleaning started at ~11:10. Cleaning cycle completes ~11:30. Drying completes ~12:00.

Assigned IDs: ESDS-QFP-AX57-G009 through ESDS-QFP-AX57-G015

1. Verify tubes are fully dry before use (wet tubes create moisture sensitivity issues).
2. Load 50 Good-bin units per tube into G009–G015 (350 units total). Exclude any units from the opened MBB (bake-out).
3. Apply GREEN label:
   `LOT-AX57 / BIN: GOOD / ESDS / MSL3-SEALED / 50 units / [Tube ID] / LMI-2025-11-20-001 / Batch 2`
4. Seal each tube.

Weight check: 7 tubes × ~1.83 lb = ~12.8 lb. PASS.

---

### Transport Trip 4 — Good Bin Batch 2 (~14:20 departure, after Trip 3 return)

**Pre-departure checklist:**
- [ ] Scrap confirmed in IMS at T-12
- [ ] Cleaned tubes verified dry and packed
- [ ] ESD wrist strap re-verified
- [ ] Escort present
- [ ] IMS sub-update: G009–G015 departing Warehouse A
- [ ] Cart loaded with G009–G015 only (~12.8 lb)
- [ ] Alternate route (restriction lifts 13:00; confirm status before departure)

**Transit and T-12 arrival:** Follow same procedure as previous trips.
- 10-minute UV cycle
- 2-minute ionizer recheck
- Wafer map and die ID scan for Good Batch 2 units
- Stage G009–G015 at T-12 intake rack alongside G001–G008 (GREEN label section)
- Update IMS: G009–G015 at T-12 Intake Rack, Cleanroom 3

Estimated Good Batch 2 staged: approximately 15:05.

---

## Section 3 — Outstanding Unresolvable Item: Container Shortage

After all four trips, 450 Good bin units remain unpackaged at Warehouse A.

- Total Good bin: 1,200 units
- Staged in Batch 1 (G001–G008): 400 units
- Staged in Batch 2 (G009–G015): 350 units
- Not staged: **450 units**

These units cannot be moved using any available container. The 22 non-ESD QFP tubes are prohibited for ESDS material regardless of urgency. Manufacturing Manager must:
1. Source 9 additional ESDS-compatible QFP tubes (from other areas of the facility, returns from testers, or procurement).
2. Authorize holding the 450 units at Warehouse A with HOLD — AWAITING ESDS CONTAINERS status in IMS.
3. Initiate a fifth transport trip once containers are available, following the same pre-move IMS, UV cycle, ionizer check, and wafer scan procedures.

Note: If any Good bin units were inside the opened MBB (bake-out), the 450 shortfall figure will increase accordingly once the affected unit count is confirmed with Operator T-12.

---

## Section 4 — Schedule Impact Notification

The 11:40–11:45 staging target cannot be met. The following SOP-required steps consume time that cannot be compressed or skipped:

| Constraint | Time Impact |
|---|---|
| ESD wrist strap verification and documentation | ~5 min |
| MSL3 bake-out coordination and MBB identification | ~10 min, then 24 h for bake-out units |
| IMS pre-move update (degraded system, 30–90 sec) | ~2 min |
| Packing + labeling + sealing all bins | ~10 min |
| UV transfer chamber (per trip) | 10 min × 4 trips = 40 min |
| Ionizer check (per trip) | 2 min × 4 trips = 8 min |
| Wafer/die scan at Cleanroom 3 (per trip) | ~8 min × 4 trips = 32 min |
| Alternate route travel (3× standard, per trip) | ~15 min × 4 trips = 60 min |
| Tube cleaning and drying (Good Batch 2) | 50 min (parallel, not on critical path for first trip) |

Earliest Good Batch 1 staging: approximately 12:05.
Full lot staging (excluding bake-out units and container shortage): approximately 15:05.
Bake-out units available: approximately 24 hours from bake-out start.
Container-shortage units: pending ESDS tube sourcing.

Manufacturing Manager, Product Engineer, and Security have been notified of the revised timeline and all blockers.

---

## Section 5 — Lot Transfer Confirmation

**Move Order:** LMI-2025-11-20-001
**Lot ID:** LOT-AX57
**Device:** QFP-128, ESDS, MSL3
**Customer:** Aerospace program
**Wafer Traceability:** Required; scanned at Cleanroom 3 scanner (T-12 vicinity) for each trip

---

### Staged at T-12 Intake Rack, Cleanroom 3

| Bin Category | Container IDs | Units | Final Location | Notes |
|---|---|---|---|---|
| Good (Batch 1) | ESDS-QFP-AX57-G001 through G008 | 400 | T-12 Intake Rack, Cleanroom 3 — GREEN section | Ionizer verified before placement. Wafer/die scan complete at CR3 scanner. |
| Good (Batch 2) | ESDS-QFP-AX57-G009 through G015 | 350 | T-12 Intake Rack, Cleanroom 3 — GREEN section | From 7 cleaned tubes (Building B). Ionizer verified. Wafer/die scan complete. Staged ~15:05. |
| Rework | ESDS-QFP-AX57-R001, R002, R003 | 150 | T-12 Intake Rack, Cleanroom 3 — YELLOW section | Physically segregated from Good. Ionizer verified. Wafer/die scan complete. Staged ~13:05. |
| Scrap | ESDS-QFP-AX57-S001 | 30 | T-12 Intake Rack, Cleanroom 3 — RED section | Physically segregated from Good and Rework. Staged ~14:05. Quality dual-verification required before scrap disposal (SOP Rule 8). |

**Total units staged at T-12:** 930 (of 1,380 total; excludes bake-out and container-shortage units)

---

### Held at Warehouse A — Awaiting ESDS Containers

| Bin Category | Container IDs | Units | Location | Status |
|---|---|---|---|---|
| Good | None assigned (unpackaged) | 450 | Warehouse A, [source shelf ID] | HOLD — AWAITING ESDS CONTAINERS. IMS flagged. Manufacturing Manager notified. Do not move. |

---

### Diverted to Bake-Out Oven Area — Not Staged

| Item | Container IDs | Units | Location | ETA |
|---|---|---|---|---|
| MSL3 seal breach material | TBD — to be confirmed with Operator T-12 | TBD | Bake-out oven area | ~24 hours from bake-out initiation (~11:10) |

Note: Unit count and bin category for bake-out material will be confirmed once Operator T-12 identifies the specific MBB and its contents. If any of these units are Good-bin devices, the 450-unit container-shortage figure increases accordingly.

---

### Quarantined — Do Not Use

| Item | Container ID | Location | Reason |
|---|---|---|---|
| Damaged ESDS QFP tube | ESDS-QFP-AX57-DMG001 | Warehouse A quarantine shelf | Hairline crack; incoming inspection flag. Quality notified. Cannot be used. |

---

## Section 6 — SOP Compliance Checklist

| Requirement | SOP Rule | Status |
|---|---|---|
| ESD wrist strap verified <1 MΩ before touching ESDS material | ESD Protection | DONE — documented at grounding station |
| Only ESDS-compatible QFP tubes used | Container Mgmt | COMPLIANT — all 19 usable tubes are ESDS-type; non-ESD tubes excluded |
| Damaged tube quarantined and Quality notified | Container Mgmt | DONE — DMG001 quarantined |
| Good, Rework, Scrap in separate sealed containers with color-coded labels | Rule 2 | COMPLIANT — G-GREEN, R-YELLOW, S-RED; sealed per protocol |
| No bin categories mixed on same cart | Rule 2 | COMPLIANT — 4 separate cart trips, one bin category per trip |
| One lot at a time (LOT-BQ90 co-transport rejected) | Rule 1 | COMPLIANT — LOT-BQ90 request rejected |
| Cart weight ≤ 50 lb per trip | Rule 7 | COMPLIANT — heaviest trip: 14.7 lb |
| MSL3 bag seal breach → bake-out initiated | Rule 3 | DONE — bake-out initiated; affected units diverted |
| High-value escort ($520K > $500K threshold) | Rule 4 | COMPLIANT — Security Officer escort confirmed before first departure |
| Finance's $480K estimate does not override LMI $520K | Rule 4 | CORRECTLY APPLIED — LMI is authoritative |
| IMS pre-move update confirmed before physical movement | Rule 10 | DONE — confirmed transaction ID logged before departure |
| UV transfer chamber 10-min cycle (each trip) | Rule 6 | COMPLIANT — 4 cycles, one per trip |
| Ionizer 2-min check before placing material at T-12 | ESD Protection | COMPLIANT — checked before each trip's placement |
| Alternate route used (main corridor 11:30–13:00 restriction) | Rule 9 | COMPLIANT — all 4 trips via alternate route |
| Wafer map and die IDs scanned (aerospace traceability) | Lot Genealogy | COMPLIANT — Cleanroom 3 scanner used; Aisle B scanner avoided (DOWN) |
| Step-skipping requests rejected | General | COMPLIANT — all SOP steps executed regardless of schedule pressure |
| 11:40–11:45 staging deadline | N/A | NOT MET — SOP requirements extend timeline to ~15:05 for stageable material; Manufacturing Manager notified |
