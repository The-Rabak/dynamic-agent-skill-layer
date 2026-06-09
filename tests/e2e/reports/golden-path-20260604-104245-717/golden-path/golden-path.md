# Stage Log: golden-path

Run ID: `golden-path-20260604-104245-717`


## Stage 00: `health_check`

**Timestamp:** 2026-06-04T10:42:45.723840506+00:00

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
    "checked_at": "2026-06-04T10:42:45.723092519Z",
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

**Timestamp:** 2026-06-04T10:42:45.861664637+00:00

### Input
```json
{
  "description": "read baseline infrastructure state before seeding"
}
```

### Output
```json
{
  "prev_graph_version": 9
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:42:45.784757308+00:00",
  "pg_graph_version": 9,
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

**Timestamp:** 2026-06-04T10:42:45.861893980+00:00

### Input
```json
{
  "scope": "global",
  "skill_md": "# harness-golden-1780569765717\ntags: golden-path, harness, e2e\n\nA harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n\n## Procedures\n- Seed a skill via the sidecar volume writer\n- Approve the pending file to trigger graph-builder pickup\n- Verify the mcp-server serves the updated graph version\n\n## Conventions\n- Always use unique slugs to prevent cross-run dedup\n- Remove seeded skills after the test to keep the volume clean\n",
  "skill_name": "harness golden 1780569765717",
  "slug": "harness-golden-1780569765717",
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

**Timestamp:** 2026-06-04T10:42:46.737689438+00:00

### Input
```json
{
  "action": "write SKILL.md.pending via sidecar",
  "scope": "Global",
  "slug": "harness-golden-1780569765717"
}
```

### Output
```json
{
  "detail": "wrote harness-golden-1780569765717/SKILL.md.pending to volume",
  "elapsed_ms": 875,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 04: `approval`

**Timestamp:** 2026-06-04T10:42:47.832632577+00:00

### Input
```json
{
  "action": "rename SKILL.md.pending -> SKILL.md",
  "scope": "global",
  "slug": "harness-golden-1780569765717"
}
```

### Output
```json
{
  "detail": "approved harness-golden-1780569765717/SKILL.md.pending → SKILL.md",
  "elapsed_ms": 1094,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 05: `snapshot_swap`

**Timestamp:** 2026-06-04T10:44:17.968671163+00:00

### Input
```json
{
  "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
  "prev_graph_version": 9,
  "timeout_secs": 90
}
```

### Output
```json
{
  "detail": "snapshot did not advance from v9 within 90s — see #156\nPG graph_version=10, served graph_version=2\nRoot cause: graph-builder bumps graph_state then errors on outbox idempotency conflict before publishing graph.rebuilt, so the mcp-server refresh subscriber never fires.",
  "elapsed_ms": 90099,
  "ok": false,
  "pg_graph_version_after": 10
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:44:17.953773452+00:00",
  "pg_graph_version": 10,
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

**Timestamp:** 2026-06-04T10:44:17.968944565+00:00

### Input
```json
{
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780569765717",
  "repo_path": "/tmp",
  "session_id": "golden-path-retrieval-1780569857968"
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

**Timestamp:** 2026-06-04T10:44:18.026588663+00:00

### Input
```json
{
  "elapsed_ms": 45,
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780569765717",
  "session_id": "golden-path-retrieval-1780569857968"
}
```

### Output
```json
{
  "additional_context_snippet": "# Compiled Context\n\nPrompt: `harness golden-path sidecar ingestion harness-golden-1780569765717`\n\n## Skill: docker-compose-service-health\n- Description: Docker Compose healthcheck patterns for Postgres, Redis, and Qdrant — wait-for-healthy sequencing in test environments\n- Score: 0.011\n\n## Skill: rust-tokio-async-file-io\n- Description: Async file I/O patterns in Rust using tokio::fs — reading, writing, and error boundaries for async contexts\n- Score: 0.011\n### Why These Skills\n- docker-compo",
  "contains_seeded_skill": true,
  "graph_version": 2,
  "latency_ms": 42,
  "reason_code": "project_scope_resolution_failed",
  "source": "retrieval",
  "status": "degraded"
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T10:44:18.014862751+00:00",
  "pg_graph_version": 10,
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

**Timestamp:** 2026-06-04T10:44:18.950465324+00:00

### Input
```json
{
  "scope": "global",
  "slug": "harness-golden-1780569765717"
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
