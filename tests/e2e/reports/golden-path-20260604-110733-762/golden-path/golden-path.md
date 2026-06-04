# Stage Log: golden-path

Run ID: `golden-path-20260604-110733-762`


## Stage 00: `health_check`

**Timestamp:** 2026-06-04T11:07:33.768477831+00:00

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
    "checked_at": "2026-06-04T11:07:33.767780554Z",
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

**Timestamp:** 2026-06-04T11:07:33.897405602+00:00

### Input
```json
{
  "description": "read baseline infrastructure state before seeding"
}
```

### Output
```json
{
  "prev_graph_version": 21
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:07:33.826250036+00:00",
  "pg_graph_version": 21,
  "pg_table_counts": {
    "communities": 5,
    "community_skills": 6,
    "outbox_events": 7,
    "skill_subunits": 13,
    "skills": 6,
    "subunits": 13,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 7
}
```

---

## Stage 02: `ingest_input`

**Timestamp:** 2026-06-04T11:07:33.897627095+00:00

### Input
```json
{
  "scope": "global",
  "skill_md": "# harness-golden-1780571253761\ntags: golden-path, harness, e2e\n\nA harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n\n## Procedures\n- Seed a skill via the sidecar volume writer\n- Approve the pending file to trigger graph-builder pickup\n- Verify the mcp-server serves the updated graph version\n\n## Conventions\n- Always use unique slugs to prevent cross-run dedup\n- Remove seeded skills after the test to keep the volume clean\n",
  "skill_name": "harness golden 1780571253761",
  "slug": "harness-golden-1780571253761",
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

**Timestamp:** 2026-06-04T11:07:34.822187994+00:00

### Input
```json
{
  "action": "write SKILL.md.pending via sidecar",
  "scope": "Global",
  "slug": "harness-golden-1780571253761"
}
```

### Output
```json
{
  "detail": "wrote harness-golden-1780571253761/SKILL.md.pending to volume",
  "elapsed_ms": 924,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 04: `approval`

**Timestamp:** 2026-06-04T11:07:35.774344077+00:00

### Input
```json
{
  "action": "rename SKILL.md.pending -> SKILL.md",
  "scope": "global",
  "slug": "harness-golden-1780571253761"
}
```

### Output
```json
{
  "detail": "approved harness-golden-1780571253761/SKILL.md.pending → SKILL.md",
  "elapsed_ms": 951,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 05: `snapshot_swap`

**Timestamp:** 2026-06-04T11:07:40.535254753+00:00

### Input
```json
{
  "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
  "prev_graph_version": 21,
  "timeout_secs": 90
}
```

### Output
```json
{
  "detail": "graph version advanced from v21 to v23; served version confirmed",
  "elapsed_ms": 4744,
  "ok": true,
  "pg_graph_version_after": 23
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:07:40.519687509+00:00",
  "pg_graph_version": 23,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 8,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 8
}
```

---

## Stage 06: `retrieval_request`

**Timestamp:** 2026-06-04T11:07:40.535548881+00:00

### Input
```json
{
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780571253761",
  "repo_path": "/tmp",
  "session_id": "golden-path-retrieval-1780571260535"
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

**Timestamp:** 2026-06-04T11:07:40.601924368+00:00

### Input
```json
{
  "elapsed_ms": 52,
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780571253761",
  "session_id": "golden-path-retrieval-1780571260535"
}
```

### Output
```json
{
  "additional_context_snippet": "# Compiled Context\n\nPrompt: `harness golden-path sidecar ingestion harness-golden-1780571253761`\n\n## Skill: harness-golden-1780571253761\n- Description: A harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n- Score: 0.011\n### Highlights\n- [procedure] Procedure note — Seed a skill via the sidecar volume writer\n\n## Skill: websocket-heartbeat-keepalive\n- Description: Keep long-lived WebSoc",
  "contains_seeded_skill": true,
  "graph_version": 23,
  "latency_ms": 49,
  "reason_code": "project_scope_resolution_failed",
  "source": "retrieval",
  "status": "degraded"
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:07:40.588982597+00:00",
  "pg_graph_version": 23,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 8,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 8
}
```

---

## Stage 08: `cleanup`

**Timestamp:** 2026-06-04T11:07:41.473277179+00:00

### Input
```json
{
  "scope": "global",
  "slug": "harness-golden-1780571253761"
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
