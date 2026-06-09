# Stage Log: golden-path

Run ID: `golden-path-20260604-110516-696`


## Stage 00: `health_check`

**Timestamp:** 2026-06-04T11:05:16.716261283+00:00

### Input
```json
{
  "url": "http://127.0.0.1:3001/health"
}
```

### Output
```json
{
  "body": {
    "checked_at": "2026-06-04T11:05:16.713666028Z",
    "components": [
      {
        "detail": "enabled",
        "healthy": true,
        "name": "usage_write"
      },
      {
        "detail": "ollama",
        "healthy": true,
        "name": "extraction_provider"
      },
      {
        "detail": "reachable",
        "healthy": true,
        "name": "postgres"
      },
      {
        "detail": "reachable",
        "healthy": true,
        "name": "redis"
      },
      {
        "detail": "reachable",
        "healthy": true,
        "name": "ollama"
      },
      {
        "detail": "reachable",
        "healthy": true,
        "name": "qdrant_write_side"
      }
    ],
    "healthy": true
  },
  "status_code": 200
}
```

### Infra Snapshot
```json
null
```

---

## Stage 01: `baseline_snapshot`

**Timestamp:** 2026-06-04T11:05:17.113502848+00:00

### Input
```json
{
  "description": "read baseline infrastructure state before seeding"
}
```

### Output
```json
{
  "prev_graph_version": 17
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:05:16.939830853+00:00",
  "pg_graph_version": 17,
  "pg_table_counts": {
    "communities": 5,
    "community_skills": 6,
    "outbox_events": 6,
    "skill_subunits": 13,
    "skills": 6,
    "subunits": 13,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 5
}
```

---

## Stage 02: `ingest_input`

**Timestamp:** 2026-06-04T11:05:17.113821003+00:00

### Input
```json
{
  "scope": "global",
  "skill_md": "# harness-golden-1780571116696\ntags: golden-path, harness, e2e\n\nA harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n\n## Procedures\n- Seed a skill via the sidecar volume writer\n- Approve the pending file to trigger graph-builder pickup\n- Verify the mcp-server serves the updated graph version\n\n## Conventions\n- Always use unique slugs to prevent cross-run dedup\n- Remove seeded skills after the test to keep the volume clean\n",
  "skill_name": "harness golden 1780571116696",
  "slug": "harness-golden-1780571116696",
  "volume": "dynamic-agent-skill-layer_test-global-skills"
}
```

### Output
```json
null
```

### Infra Snapshot
```json
null
```

---

## Stage 03: `sidecar_write`

**Timestamp:** 2026-06-04T11:05:18.086022811+00:00

### Input
```json
{
  "action": "write SKILL.md.pending via sidecar",
  "scope": "Global",
  "slug": "harness-golden-1780571116696"
}
```

### Output
```json
{
  "detail": "wrote harness-golden-1780571116696/SKILL.md.pending to volume",
  "elapsed_ms": 971,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 04: `approval`

**Timestamp:** 2026-06-04T11:05:19.070551619+00:00

### Input
```json
{
  "action": "rename SKILL.md.pending -> SKILL.md",
  "scope": "global",
  "slug": "harness-golden-1780571116696"
}
```

### Output
```json
{
  "detail": "approved harness-golden-1780571116696/SKILL.md.pending → SKILL.md",
  "elapsed_ms": 983,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 05: `snapshot_swap`

**Timestamp:** 2026-06-04T11:05:20.207245437+00:00

### Input
```json
{
  "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
  "prev_graph_version": 17,
  "timeout_secs": 90
}
```

### Output
```json
{
  "detail": "graph version advanced from v17 to v19; served version confirmed",
  "elapsed_ms": 1121,
  "ok": true,
  "pg_graph_version_after": 19
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:05:20.192195501+00:00",
  "pg_graph_version": 19,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 7,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 6
}
```

---

## Stage 06: `retrieval_request`

**Timestamp:** 2026-06-04T11:05:20.207506323+00:00

### Input
```json
{
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780571116696",
  "repo_path": "/tmp",
  "session_id": "golden-path-retrieval-1780571120207"
}
```

### Output
```json
null
```

### Infra Snapshot
```json
null
```

---

## Stage 07: `retrieval_response`

**Timestamp:** 2026-06-04T11:05:20.273234925+00:00

### Input
```json
{
  "elapsed_ms": 52,
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780571116696",
  "session_id": "golden-path-retrieval-1780571120207"
}
```

### Output
```json
{
  "additional_context_snippet": "# Compiled Context\n\nPrompt: `harness golden-path sidecar ingestion harness-golden-1780571116696`\n\n## Skill: harness-golden-1780571116696\n- Description: A harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n- Score: 0.011\n### Highlights\n- [procedure] Procedure note — Seed a skill via the sidecar volume writer\n\n## Skill: rust-tokio-async-file-io\n- Description: Async file I/O patterns in ",
  "contains_seeded_skill": true,
  "graph_version": 19,
  "latency_ms": 49,
  "reason_code": "project_scope_resolution_failed",
  "source": "retrieval",
  "status": "degraded"
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:05:20.260712013+00:00",
  "pg_graph_version": 19,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 7,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 6
}
```

---

## Stage 08: `cleanup`

**Timestamp:** 2026-06-04T11:05:21.317673836+00:00

### Input
```json
{
  "scope": "global",
  "slug": "harness-golden-1780571116696"
}
```

### Output
```json
{
  "detail": "Ok(())",
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---
