# InfoAID Response: DryWave 3000 — Multi-Room Humidity Stabilisation Strategy

> **Format notes:**
> - A `.docx` file cannot be generated in this environment; this response is written as `solution.md`.
> - The customer requested two paragraphs, but the required InfoAID response structure (Summary, Steps, Definitions, Warnings, Assumptions) and the multi-part safety-critical nature of the query cannot be compressed to two paragraphs without omitting material procedural and hazard information. The full structure is provided below to protect the customer and ensure correct deployment.

---

## Summary / Answer

With two DryWave 3000 units serving three rooms across a three-storey property, the deployment strategy assigns each unit a primary responsibility aligned to the highest sustained moisture risk, with Unit 2 rotating to cover the episodic burst-load of the laundry room. **Unit 1 (DryWave 3000-P recommended)** is permanently deployed in the **basement**, where earth-moisture infiltration demands the strongest fan torque, continuous drainage, and the ability to run at full duty (§37.3). It begins in **CONTINUOUS mode** for initial stabilisation and switches to **AUTO mode** once RH falls below 55% (§20.3). Target RH: **45–48%** — the mould-prevention band for damp-prone rooms (§5) within the basement/cellar range of 40–50% (§13). **Unit 2 (DryWave 3000-H recommended)** is primarily deployed in the **top-floor archive library** in **AUTO mode** at **48–50% RH** (§5, §13, §40). During scheduled laundry-drying sessions, Unit 2 is temporarily relocated to the **ground-floor laundry room** in **CONTINUOUS mode** targeting **40% or lower** (§5, §13, §20.5), then returned to the archive. This rotational approach satisfies the multi-unit principle that "each unit should be responsible for a logical air segment, rather than all devices competing for the same air mass" (§28).

---

## Placement Summary Table

| Room | Floor | Unit | Model | Initial Mode | Steady-State Mode | Target RH | Key Placement Rules | Drainage |
|---|---|---|---|---|---|---|---|---|
| Basement | Lower | Unit 1 | 3000-P | CONTINUOUS | AUTO (after RH <55%) | 45–48% | Central open area; elevated from floor; ≥20 cm clearance all sides; no corners/alcoves; away from masonry drips (§7, §11) | Continuous drainage mandatory (§15) |
| Laundry Room | Ground | Unit 2 (temporary) | 3000-H | CONTINUOUS | CONTINUOUS (active drying only) | 40% or lower | Adjacent to drying zone; not behind appliances; clear exhaust path (§20.5, §11) | Continuous drainage if available (§15) |
| Archive Library | Top | Unit 2 (primary) | 3000-H | AUTO | AUTO | 48–50% | Open aisle, not behind shelving; ≥20 cm clearance; not opposite forced-air vents; doors kept closed (§7, §10, §11) | Tank mode acceptable (low sustained extraction) |

---

## Steps / Procedure

### Phase 1 — Initial Stabilisation

1. **Basement — Unit 1:** Connect continuous drainage hose before first activation (§15 — "basement conditions" listed as a primary use case for continuous drainage). Route hose to a drain with no upward bends; confirm outlet is below unit base level (§21). Position unit in an open central floor area, elevated slightly from the floor where possible (§11), with ≥20 cm clearance to all walls and no furniture obstructions (§7). Do not place in corners, alcoves, or beneath objects (§11). Keep the basement door closed (§5, §10). Set to **CONTINUOUS mode**; target **45–48% RH**. The manual specifies 24–96 hours for humidity normalisation in basements (§22); run CONTINUOUS until RH stabilises below 55%.

2. **Archive Library — Unit 2:** Position in the open airflow mixing zone of the library — not behind shelving rows, which create air dead-zones (§10). Maintain ≥20 cm clearance to walls and shelving (§7). Do not place directly opposite forced-air vents or heat registers (§11). Keep library doors closed (§5). Set to **AUTO mode**; target **48–50% RH** (§5 — "48–50% for archive rooms"; §40 — "Set unit to maintain RH ~49%").

### Phase 2 — Laundry Room Cycles

3. **Before laundry drying:** Relocate Unit 2 from archive to ground-floor laundry room. Position **directly adjacent to the drying zone** (§20.5). Connect continuous drainage if a drain point is available (§15). Set to **CONTINUOUS mode**; target **40% or lower** (§13 — "Laundry drying: 40% or lower"). Run until fabrics are dry and room RH has recovered toward target.

4. **Post-laundry return:** Carry unit upright using handle (§43). If unit was tilted beyond 55° during transport, allow 60 minutes standing before reactivation (§43). Return Unit 2 to archive library. Resume **AUTO mode** at 48–50% target.

### Phase 3 — Steady-State Operation

5. Once the basement sustains RH below 55%, switch Unit 1 to **AUTO mode** (§20.3 — "then AUTO mode once RH is below 55%"). AUTO conserves energy while dynamically maintaining target band (§12).

6. For long unattended periods: confirm tank is empty or drainage is connected; filters are clean; unit is securely positioned; ventilation is unobstructed (§24). Use AUTO mode rather than CONTINUOUS for extended travel periods unless conditions require maximum extraction (§24).

### Seasonal Adjustments (§29)

| Season | Primary Risk in This Property | Recommended Adjustment |
|---|---|---|
| **Winter** | Cold external temperatures drive condensation on basement masonry and archive exterior walls | Maintain AUTO at ~50% RH in both rooms (§29); inspect basement for surface condensation weekly; verify drainage hose is not obstructed by cold |
| **Spring** | Transitional fluctuations as heating patterns change | Run daily extraction cycles in both rooms until weather stabilises (§29) |
| **Summer** | Warm outdoor air carries higher absolute vapour; basement vapour ingress rate increases | Hold archive and basement at 45–50% (§29); monitor Unit 1 — tank may fill faster; continuous drainage essential |
| **Autumn** | Rain and cooler temperatures raise condensation risk on all surfaces | Lengthen ACTIVE extraction periods (§29); temporarily switch basement Unit 1 back to CONTINUOUS mode during heavy rainfall periods |

---

## Definitions

- **RH (Relative Humidity):** Percentage of water vapour in air relative to its saturation point at a given temperature. Above ~60% RH, condensation forms on cold surfaces and microbial growth accelerates (§2).
- **AUTO mode:** Dynamic extraction that continuously adjusts motor speed and coil load to maintain the user-set target RH. Energy-efficient; suitable for regular daily conditioning and long unattended operation (§12).
- **QUIET mode:** Reduced airflow; 32–34 dB noise emission. Not suitable as the primary mode in high-humidity environments because extraction is slower (§12, §32). **Not used in this deployment.**
- **CONTINUOUS mode:** Maximum-capability extraction at full duty until manually disabled or tank cutoff. Used for initial remediation, laundry drying, and post-rainfall recovery (§12).
- **Airflow short-circuiting:** Condition where exhaust air immediately re-enters the intake grille, causing inaccurate humidity readings and inefficient cycling. Prevented by maintaining ≥20 cm clearance on all sides and avoiding corner or alcove placement (§7).
- **Float-trigger threshold:** The water level at which the internal float sensor activates automatic shutdown to prevent tank overflow (§9).
- **DryWave 3000-P:** Professional variant with reinforced condensation tolerance and dual-ball-race bearing; rated for full-duty continuous operation; recommended for basements and high-demand loads (§37.3).
- **DryWave 3000-H:** HEPA-13 equipped variant; controls micro-particulates, fungal spores, and allergens; recommended for archive environments where biological particulate control protects stored materials (§37.2).
- **Continuous drainage:** Direct hose connection from unit to a drain, bypassing the internal tank; mandatory for unattended sustained operation (§15, §21).

---

## Warnings / Safety

1. **Continuous drainage is mandatory for unattended operation.** The unit automatically shuts down when the internal tank reaches the float-trigger threshold (§9). In the basement and laundry room, where extraction rates are high, the 3.5-litre tank will fill rapidly — especially in summer or after rainfall. Without continuous drainage, the unit halts and humidity immediately begins to rise again. Connect and test the drainage hose before leaving the unit unattended (§24).

2. **Motor thermal protection trips at 78°C.** If the interior motor coil exceeds 78°C, the unit shuts down automatically (§9). The primary cause is CONTINUOUS mode operation with a partially saturated filter, which raises motor load (Annex E: overheat P = 0.00027/hour, elevated with dusty filter). **Clean the micro-mesh filter monthly** (§17). Proper filter maintenance raises MTBF from approximately 1,886 hours to approximately 5,263 hours (Annex E).

3. **Do not obstruct airflow.** Units placed behind furniture, inside alcoves, beneath tables, or in corners with less than 20 cm clearance will short-circuit their exhaust back into the intake, producing false humidity readings and reduced extraction (§7, §11). In the archive library, bookshelves create air dead-zones; position Unit 2 in an open aisle.

4. **Invalid sensor readings trigger automatic shutdown.** If the humidity sensor returns out-of-range values, the unit halts and enters standby (§9). Sensor accuracy is ±2% (§8) with a long-term drift rate below 0.03% per month (§39), so normal readings remain reliable. However, in dusty environments such as an archive library, the sensor chamber should be cleaned every 12–18 months by a qualified technician (§8).

5. **Dedicated wall socket required.** Each unit must be plugged into a directly wired wall socket — not into multi-port extension strips shared with other heavy appliances (§6, §19). Verify that the basement and archive library each have a suitable dedicated socket before deployment.

6. **Never operate a humidifier concurrently with a dehumidifier in the same enclosed space** (§33). If any humidifier is present elsewhere in the property, confirm it is isolated from the rooms being treated.

7. **Unit transport:** When relocating Unit 2 between archive and laundry, maintain upright orientation. If the unit is tilted beyond 55°, allow 60 minutes standing before reactivation to allow internal fluid to redistribute (§43). Empty the tank before moving.

8. **Do not target below 40% RH in living areas.** Manual §13 states that maintaining humidity below 40% for extended periods is not recommended for domestic spaces, as prolonged low-moisture conditions cause human discomfort and may cause wood to contract or crack. In the laundry room, CONTINUOUS mode should be stopped once laundry is dry and RH begins to recover.

9. **Below 5°C, condensation efficiency declines** (§5). If the basement temperature drops below 5°C in winter, extraction capacity is reduced. Monitor RH manually during cold spells and consider supplemental heating if the space allows.

---

## Failure Modes and Corrective Actions

| Failure Mode | Relevant Sections | Most Likely Cause | Corrective Action |
|---|---|---|---|
| Basement humidity not decreasing despite CONTINUOUS operation | §27, §5 | Clogged micro-mesh filter; earth-moisture ingress rate exceeds extraction; basement door left open | Clean filter immediately (§17); close and seal basement door; verify drainage hose has no upward bends or kinks; reposition unit away from masonry-contact air-dead zone |
| Unit 1 shuts down intermittently | §27, §9 | Tank filling (drainage hose blocked or absent); thermal protection activating due to filter clog; voltage irregularity at socket | Inspect and clear drainage hose; clean filter; verify direct wall socket connection; improve air circulation around unit |
| Archive RH drifts outside 48–50% during laundry rotation | §27, §16 | Unit 2 absent during extended or back-to-back drying sessions; humid stairwell air migrating under archive door | Keep all intermediate stairwell doors closed during laundry cycles; limit Unit 2 absence to the drying period only; return unit to archive promptly |
| Water leakage from tank area | §27, §15 | Tank overfilled due to improper seating; drainage hose with upward bend causing backflow; tank seal contamination | Inspect and reseat tank; re-route drainage hose to eliminate all upward bends (§21); clean tank seal; level unit on stable surface |
| Unusual motor hum or vibration | §27, §32 | Unit on hollow or resonant floor surface; loose tank; particulate debris contacting fan blades | Place unit on dense substrate or acoustic mat (§32); reseat tank; inspect intake grille for debris; schedule technician if persistent (bearing wear on 3000-P in long operation) |
| Sensor returns invalid readings / shutdown | §9, §8 | Sensor chamber contaminated by archive dust or spores | Schedule technician sensor chamber cleaning (§8 — every 12–18 months); increase HEPA filter inspection frequency to reduce particulate load reaching sensor |
| Both units unavailable simultaneously (e.g., Unit 1 faulty, Unit 2 at laundry) | §22, §29 | Unplanned concurrent failure during laundry cycle | Establish a rule: Unit 2 returns to archive before laundry exceeds 4 hours; keep Unit 1 maintained to manufacturer schedule; during winter and autumn, prioritise archive protection over laundry speed |

---

## Assumptions

1. **The archive library has a closeable door.** The manual requires doors and windows to be closed during extraction cycles for effective treatment of the contained air volume (§5, §10). If the archive is open-plan, humidity control will be significantly slower and less stable.

2. **Continuous drainage access exists in the basement.** The unit requires a drainage endpoint at a lower elevation than its base, with no upward bends and a run no longer than 3.2 metres unless gravity drop assists (§21). If no drain is available, the 3.5-litre tank must be manually emptied — unattended overnight operation is then impractical given potential extraction volumes.

3. **The three rooms are on separate floors connected by a stairwell.** This satisfies the multi-unit requirement that "each unit should be placed in a different humidity concentration zone" (§28). If the rooms share the same air mass (open stairs, open doors), the humidity profile of each zone will be influenced by the others.

4. **Ambient temperature in all rooms remains between 5°C and 32°C** (§5). If the basement drops below 5°C in winter, condensation efficiency will decline and active monitoring is required. If any room exceeds 32°C in summer, compressor workload increases.

5. **Laundry drying sessions are scheduled**, not continuous. This plan depends on Unit 2 returning to the archive library within hours of a laundry cycle completing. If laundry drying is frequent and extended (e.g., multiple loads per day), a third unit dedicated to the laundry room would be necessary to maintain uninterrupted archive protection.

6. **The DryWave 3000-P and 3000-H model variants are available.** If only the standard 3000-S is available, basement performance will be reduced — the 3000-S has lower fan torque and is not rated for full-duty continuous operation (§37.1 vs §37.3). The 3000-S is sized for rooms up to ~45 m²; if the basement exceeds this, extraction efficiency will be further limited (Annex G).
