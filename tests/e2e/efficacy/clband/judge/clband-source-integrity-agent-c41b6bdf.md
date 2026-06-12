# Judge prompt (SECONDARY score) — clband-source-integrity-agent-c41b6bdf

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are the Source Integrity Agent (SIA) in a multi-agent news-verification pipeline. Follow the SIA system's operational protocols, JSON output schema, Domain Trust Score formula, and escalation rules exactly.

[Turn 1 - 2025-11-16T12:10:03Z] Message from: Orchestrator
We are fast-tracking a source integrity check for a link going into tonight's brief. AO has bundled all technical captures. Please review the handoff and artifacts for the final integrity packet.
[... artifact bundle attached; see task spec for full prompt ...]

Final Task: Produce the single, final source integrity analysis for the provided URL and bundle. Use only the technical evidence in the artifacts and transcript. Include the domain trust scoring basis, explicit timestamp comparison with delta, metadata integrity findings with missing fields, spoofing risk assessment with evidence, any required escalation and reason, limitations, and routing notes for downstream teams. Deliver one complete structured output.

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. Does the output follow the exact structured JSON format specified in the "Required Output Structure"? Fail if output is not valid JSON or deviates from the required structure
2. Are all fields from the required JSON structure included, even if their value is "N/A"? Fail if any required key is missing from the final JSON output
3. Does SIA avoid assessing the factual correctness or accuracy of the content? Fail if SIA's output contains any statement or judgment about the factual accuracy of the source content
4. Does SIA refrain from inferring or estimating missing metadata values? Fail if SIA attempts to calculate, infer, or guess a value for any missing metadata field
5. Does SIA only use the specified validation checks (WHOIS, DNS, EXIF, timestamps)? Fail if SIA mentions or uses a validation method not explicitly listed in the document
6. Does SIA first validate metadata completeness and list any missing fields? Fail if the validation and listing of missing metadata is not performed as the first step after receiving input
7. Does SIA use the exact formula for the Domain Trust Score calculation? Fail if the calculated Trust Score does not match the result from the specified formula
8. Does SIA verify domain age, registrar, and nameserver consistency? Fail if the output does not contain findings related to domain age, registrar, or nameserver consistency
9. Does SIA compare claimed vs server-side timestamps? Fail if the output does not contain a comparison and delta calculation between server and metadata timestamps
10. Does SIA attempt to identify cloaking, forged headers, and DNS mismatches? Fail if the "spoofing_detection" object does not contain checks for cloaking, DNS mismatch, or forged headers
11. Does every finding in the output have explicit evidence? Fail if any finding, status, or conclusion lacks specific, quoted technical evidence
12. If metadata is missing, does SIA reduce the trust score by 10 points? Fail if Trust Score is not reduced by 10 points for missing metadata
13. If WHOIS privacy protection is detected, does SIA reduce the trust score by 15 points? Fail if Trust Score is not reduced by 15 points for WHOIS privacy protection
14. Does SIA trigger an escalation if the timestamp delta is $>7$ days? Fail if a timestamp delta $>7$ days is detected and no escalation is triggered
15. When a server timestamp $\neq$ EXIF timestamp, does SIA report both values and calculate the delta? Fail if SIA reports only one timestamp value or fails to calculate the delta when a conflict exists
16. When multiple conflicting timestamps are found in metadata, does SIA list all values and flag the inconsistency? Fail if SIA selects one timestamp as "best guess" or fails to list all conflicting values
17. Does SIA flag a conflict if the server timestamp $\neq$ EXIF timestamp delta is $>24\text{h}$? Fail if the timestamp delta is $>24\text{h}$ but the inconsistency is not flagged
18. Are all timestamps in the output formatted using ISO 8601? Fail if any timestamp value in the JSON output is not in ISO 8601 format
19. Does the output use the exact required field names (e.g., "domain_trust_score", "metadata_validation", "spoofing_detection")? Fail if any of the top-level keys in the required JSON structure are misspelled or paraphrased
20. Does the output include the "missing_metadata_fields" list when fields are absent? Fail if metadata is missing but the "missing_metadata_fields" key is empty or absent
21. Does SIA preserve the original field names from the source metadata in its report (e.g., "DateTimeOriginal", not "Creation Date")? Fail if SIA changes the original field names of source metadata being reported
22. Does SIA explicitly state "MISSING" for any unavailable metadata field? Fail if an unavailable metadata field is represented by "ABSENT", "None", or an empty string instead of "MISSING"
23. Does the "domain_trust_score" object include "score", "calculation_basis", and "justification"? Fail if any of the three required sub-keys are missing from "domain_trust_score"
24. Does the "metadata_validation.status" use exactly "PASS | FAIL | INCOMPLETE"? Fail if "metadata_validation.status" is any other value (e.g., "OK", "Partial")
25. Does the "timestamp_match" result use exactly "MATCH | MISMATCH | MISSING"? Fail if the "timestamp_match" result is any other value
26. Does the "EXIF_integrity" result use exactly "INTACT | TAMPERED | ABSENT"? Fail if the "EXIF_integrity" result is any other value
27. Does the "spoofing_detection.risk_level" use exactly "NONE | LOW | MEDIUM | HIGH | CRITICAL"? Fail if the "risk_level" is any other value
28. Does the output include the "next_agent_notes" object with keys "to_FCA" and "to_AO"? Fail if "next_agent_notes" is missing or does not contain both "to_FCA" and "to_AO" keys
29. Does SIA avoid vague, subjective language? Fail if SIA uses subjective or vague phrases instead of specific technical findings (e.g., "looks suspicious")
30. When conflicts arise, does SIA report all conflicting values and flag the discrepancy without resolving the conflict? Fail if SIA attempts to resolve a reported conflict by selecting a single "most likely" value
31. Does the DNS Consistency check ensure both nameserver consistency and expected IP address resolution? Fail if only nameservers are checked, but the resolved IP is not compared to the expected/claimed server
32. Is the `Domain_Age_Score` scaled logically such that a very recently registered domain (e.g., $<30$ days) results in a low score? Fail if a domain registered $<30$ days ago is assigned a high `Domain_Age_Score`

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
