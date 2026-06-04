# Stage Log: golden-path

Run ID: `golden-path-20260604-100223-117`


## Stage 00: `health_check`

**Timestamp:** 2026-06-04T10:02:23.124087891+00:00

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
    "checked_at": "2026-06-04T10:02:23.123236387Z",
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

**Timestamp:** 2026-06-04T10:02:23.259679660+00:00

### Input
```json
{
  "description": "read baseline infrastructure state before seeding"
}
```

### Output
```json
{
  "prev_graph_version": 7
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:02:23.183167528+00:00",
  "pg_graph_version": 7,
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
  "redis_stream_len": 1
}
```

---

## Stage 02: `ingest_input`

**Timestamp:** 2026-06-04T10:02:23.259915695+00:00

### Input
```json
{
  "scope": "global",
  "skill_md": "# harness-golden-1780567343117\ntags: golden-path, harness, e2e\n\nA harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n\n## Procedures\n- Seed a skill via the sidecar volume writer\n- Approve the pending file to trigger graph-builder pickup\n- Verify the mcp-server serves the updated graph version\n\n## Conventions\n- Always use unique slugs to prevent cross-run dedup\n- Remove seeded skills after the test to keep the volume clean\n",
  "skill_name": "harness golden 1780567343117",
  "slug": "harness-golden-1780567343117",
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

**Timestamp:** 2026-06-04T10:02:24.121842692+00:00

### Input
```json
{
  "action": "write SKILL.md.pending via sidecar",
  "scope": "Global",
  "slug": "harness-golden-1780567343117"
}
```

### Output
```json
{
  "detail": "wrote harness-golden-1780567343117/SKILL.md.pending to volume",
  "elapsed_ms": 856,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 04: `approval`

**Timestamp:** 2026-06-04T10:02:25.072515837+00:00

### Input
```json
{
  "action": "rename SKILL.md.pending -> SKILL.md",
  "scope": "global",
  "slug": "harness-golden-1780567343117"
}
```

### Output
```json
{
  "detail": "approved harness-golden-1780567343117/SKILL.md.pending → SKILL.md",
  "elapsed_ms": 950,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 05: `snapshot_swap`

**Timestamp:** 2026-06-04T10:03:55.241572811+00:00

### Input
```json
{
  "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
  "prev_graph_version": 7,
  "timeout_secs": 90
}
```

### Output
```json
{
  "detail": "snapshot did not advance from v7 within 90s — see #156\nPG graph_version=8, served graph_version=2\nRoot cause: graph-builder bumps graph_state then errors on outbox idempotency conflict before publishing graph.rebuilt, so the mcp-server refresh subscriber never fires.",
  "elapsed_ms": 90129,
  "ok": false,
  "pg_graph_version_after": 8
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:03:55.226523973+00:00",
  "pg_graph_version": 8,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 6,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 1
}
```

---

## Stage 06: `retrieval_request`

**Timestamp:** 2026-06-04T10:03:55.241850945+00:00

### Input
```json
{
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780567343117",
  "repo_path": "/tmp",
  "session_id": "golden-path-retrieval-1780567435241"
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

**Timestamp:** 2026-06-04T10:03:55.305227453+00:00

### Input
```json
{
  "elapsed_ms": 50,
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780567343117",
  "session_id": "golden-path-retrieval-1780567435241"
}
```

### Output
```json
{
  "additional_context_snippet": "# Compiled Context\n\nPrompt: `harness golden-path sidecar ingestion harness-golden-1780567343117`\n\n## Skill: docker-compose-service-health\n- Description: Docker Compose healthcheck patterns for Postgres, Redis, and Qdrant — wait-for-healthy sequencing in test environments\n- Score: 0.011\n\n## Skill: rust-tokio-async-file-io\n- Description: Async file I/O patterns in Rust using tokio::fs — reading, writing, and error boundaries for async contexts\n- Score: 0.011\n### Why These Skills\n- docker-compo",
  "contains_seeded_skill": true,
  "graph_version": 2,
  "latency_ms": 47,
  "reason_code": "project_scope_resolution_failed",
  "source": "retrieval",
  "status": "degraded"
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:03:55.293211038+00:00",
  "pg_graph_version": 8,
  "pg_table_counts": {
    "communities": 6,
    "community_skills": 7,
    "outbox_events": 6,
    "skill_subunits": 18,
    "skills": 7,
    "subunits": 18,
    "transcript_ingest_queue": 1
  },
  "qdrant_points_count": 0,
  "qdrant_status": "green",
  "redis_stream_len": 1
}
```

---

## Stage 08: `cleanup`

**Timestamp:** 2026-06-04T10:03:56.214057345+00:00

### Input
```json
{
  "scope": "global",
  "slug": "harness-golden-1780567343117"
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
