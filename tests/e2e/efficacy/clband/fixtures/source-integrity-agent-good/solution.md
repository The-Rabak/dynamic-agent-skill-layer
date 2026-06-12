```json
{
  "agent": "SIA",
  "timestamp": "2025-11-16T12:28:00Z",
  "source_url": "https://news-briefs.today/markets/alpha-report-claim",

  "domain_trust_score": {
    "score": 43,
    "calculation_basis": ["WHOIS age", "SSL validity", "DNS consistency", "Metadata integrity"],
    "justification": "Domain_Age_Score: domain registered 2025-10-31 (16 days ago, <30 days), low age score=5. SSL_Validity: certificate valid (NotBefore 2025-08-20, NotAfter 2026-08-19, OCSP GOOD), score=100. DNS_Consistency: resolved IP 203.0.113.45 matches expected origin 203.0.113.45, score=100. Metadata_Integrity: GPSLatitude absent, GPSLongitude absent, Software absent — deducting 10 points for missing metadata, score=60. Base calculation: (5x0.4)+(100x0.3)+(100x0.2)+(60x0.1)=2+30+20+6=58. WHOIS privacy protection (PrivacyProtect.org) detected — applying -15 points deduction per SIA rules. Final trust score=43."
  },

  "metadata_validation": {
    "status": "INCOMPLETE",
    "findings": [
      {
        "check": "timestamp_match",
        "result": "MISMATCH",
        "details": "server Date header: 2025-11-15T12:30:15Z; EXIF DateTimeOriginal: 2025-11-03T08:12:00; delta: 12 days 4 hours 18 minutes (>7 days — escalation triggered). Additional conflicts: XMP CreateDate: 2025-10-28T21:12:00-07:00, FileModifiedDate: 2025-11-10T09:00:00Z. All three timestamp values reported without consolidation."
      },
      {
        "check": "EXIF_integrity",
        "result": "INTACT",
        "evidence": "EXIF fields present as captured: DateTimeOriginal=2025:11:03 08:12:00, Make=Canon, Model=Canon EOS 80D. No anomalies in available fields."
      }
    ],
    "missing_metadata_fields": ["GPSLatitude", "GPSLongitude", "Software", "SubSecTimeOriginal"],
    "raw_missing_field_values": {
      "GPSLatitude": "MISSING",
      "GPSLongitude": "MISSING",
      "Software": "MISSING",
      "SubSecTimeOriginal": "MISSING"
    }
  },

  "spoofing_detection": {
    "risk_level": "LOW",
    "indicators": ["WHOIS privacy protection — registrant identity unverifiable"],
    "evidence": "DNS resolves to 203.0.113.45 (Artifact 2, resolvers 8.8.8.8 and 1.1.1.1 agree), matching expected origin IP 203.0.113.45. No DNS mismatch. No forged origin headers in Artifact 6. No cloaking: displayed URL matches actual destination."
  },

  "escalation_required": true,
  "escalation_reason": "timestamp_tampering",

  "limitations": [
    "WHOIS privacy protection (PrivacyProtect.org) prevents registrant ownership verification",
    "EXIF DateTimeOriginal lacks explicit timezone — camera local time assumed per Artifact 3 note",
    "Junior analyst precheck (Artifact 7) relied on 2020 PDF snippet; excluded from evidence"
  ],

  "next_agent_notes": {
    "to_FCA": "Trust score 43/100 — proceed with heightened scrutiny. Timestamp delta 12+ days between server Date and EXIF DateTimeOriginal exceeds 7-day escalation threshold. Multiple conflicting timestamps (EXIF: 2025-11-03, XMP: 2025-10-29, FileModified: 2025-11-10) reported without consolidation. Domain registered 16 days ago. WHOIS privacy-protected.",
    "to_AO": "Escalation required: timestamp delta >7 days (server 2025-11-15 vs EXIF 2025-11-03, delta ~12 days). Human Security Reviewer recommended per SIA escalation protocol."
  }
}
```
