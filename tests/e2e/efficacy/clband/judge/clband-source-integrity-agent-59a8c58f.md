# Judge prompt (SECONDARY score) — clband-source-integrity-agent-59a8c58f

The deterministic verifier decides pass/fail. This judge score is reported as a secondary,
all-rubrics-must-pass CL-bench-native reading only. Rubrics are VERBATIM from the pinned
dataset. Reference assistant answers (if any existed in the context) are NEVER shown to any
solve arm — they are verifier/judge authoring material only.

## Task prompt given to the agent
```
You are the Source Integrity Agent (SIA) in a multi-agent news-verification pipeline. Follow the SIA system's operational protocols, JSON output schema, Domain Trust Score formula, and escalation rules exactly.

[Turn 1 - 2025-11-29T07:45:02Z] Message from: Orchestrator
We received a news link from the social team that needs a source integrity gate before fact checking. The URL came through a link shortener in a partner post. We need a go or no-go on the source itself.
[... artifact bundle attached; see task spec for full prompt ...]

Final Task: Using the URL and materials above, produce the source integrity assessment. Use only WHOIS, DNS, SSL, headers, and metadata provided. Compute the domain trust score using the specified weighted inputs and reflect any deductions from expired SSL, missing metadata, or privacy-protected registration. Cross-check server-side times against page and media timestamps, report the delta, and list all conflicting values without selecting one. Evaluate spoofing indicators including cloaking, DNS mismatch, and header anomalies. Include explicit evidence for each finding, enumerate any missing metadata fields using their original names, and preserve all original field names when referenced. Provide clear routing to the next stage and indicate whether any escalation is required. Return the result in the required JSON structure only.

Write your complete response to a file named `solution.md` in the current working directory. Output only that file.
```

## Rubrics (verbatim — ALL must pass for a judge WIN)
1. Does the output follow the exact structured JSON format specified in the "Required Output Structure"? Fail if output is not valid JSON or deviates from the required structure
2. Are all fields from the required JSON structure included, even if their value is "N/A"? Fail if any required key is missing from the final JSON output
3. Does SIA first validate metadata completeness and list any missing fields? Fail if the validation and listing of missing metadata is not performed as the first step after receiving input
4. Does SIA use the exact formula for the Domain Trust Score calculation? Fail if the calculated Trust Score does not match the result from the specified formula
5. Does SIA compare claimed vs server-side timestamps? Fail if the output does not contain a comparison and delta calculation between server and metadata timestamps
6. Does SIA attempt to identify cloaking, forged headers, and DNS mismatches? Fail if the "spoofing_detection" object does not contain checks for cloaking, DNS mismatch, or forged headers
7. If the SSL certificate is expired, does SIA reduce the trust score by 30 points? Fail if Trust Score is not reduced by 30 points for an expired SSL certificate
8. Does SIA trigger an escalation if the timestamp delta is $>7$ days? Fail if a timestamp delta $>7$ days is detected and no escalation is triggered
9. Does the output include the "missing_metadata_fields" list when fields are absent? Fail if metadata is missing but the "missing_metadata_fields" key is empty or absent
10. Does SIA preserve the original field names from the source metadata in its report (e.g., "DateTimeOriginal", not "Creation Date")? Fail if SIA changes the original field names of source metadata being reported
11. Does the "EXIF_integrity" result use exactly "INTACT | TAMPERED | ABSENT"? Fail if the "EXIF_integrity" result is any other value
12. Does the "spoofing_detection.risk_level" use exactly "NONE | LOW | MEDIUM | HIGH | CRITICAL"? Fail if the "risk_level" is any other value
13. Does the output include the "next_agent_notes" object with keys "to_FCA" and "to_AO"? Fail if "next_agent_notes" is missing or does not contain both "to_FCA" and "to_AO" keys
14. Does SIA flag a DNS mismatch (DNS resolves to different IP than claimed) as HIGH RISK spoofing? Fail if a DNS mismatch is detected but is not marked as HIGH RISK spoofing
15. Does SIA mark an expired SSL certificate as "EXPIRED"? Fail if an expired SSL certificate is not marked as "EXPIRED" in the output
16. Does the DNS Consistency check ensure both nameserver consistency and expected IP address resolution? Fail if only nameservers are checked, but the resolved IP is not compared to the expected/claimed server

## Instruction to the judge
You are grading a single response against the rubrics above. For EACH rubric, output
PASS or FAIL with a one-line justification quoting the response. Then output a final line
`JUDGE: WIN` only if every rubric is PASS, else `JUDGE: LOSS`. Judge knowledge only from
the rubrics; do not invent requirements.
