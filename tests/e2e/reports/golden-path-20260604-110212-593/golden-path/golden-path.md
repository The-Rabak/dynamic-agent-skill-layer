# Stage Log: golden-path

Run ID: `golden-path-20260604-110212-593`


## Stage 00: `health_check`

**Timestamp:** 2026-06-04T11:02:12.600120267+00:00

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
    "checked_at": "2026-06-04T11:02:12.599358829Z",
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

**Timestamp:** 2026-06-04T11:02:12.733867500+00:00

### Input
```json
{
  "description": "read baseline infrastructure state before seeding"
}
```

### Output
```json
{
  "prev_graph_version": 13
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:02:12.661202498+00:00",
  "pg_graph_version": 13,
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
  "redis_stream_len": 3
}
```

---

## Stage 02: `ingest_input`

**Timestamp:** 2026-06-04T11:02:12.734085368+00:00

### Input
```json
{
  "scope": "global",
  "skill_md": "# harness-golden-1780570932593\ntags: golden-path, harness, e2e\n\nA harness-seeded skill for the golden-path E2E test. This skill demonstrates correct sidecar ingestion, graph rebuild, and retrieval for the harness tracer bullet.\n\n## Procedures\n- Seed a skill via the sidecar volume writer\n- Approve the pending file to trigger graph-builder pickup\n- Verify the mcp-server serves the updated graph version\n\n## Conventions\n- Always use unique slugs to prevent cross-run dedup\n- Remove seeded skills after the test to keep the volume clean\n",
  "skill_name": "harness golden 1780570932593",
  "slug": "harness-golden-1780570932593",
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

**Timestamp:** 2026-06-04T11:02:13.668090217+00:00

### Input
```json
{
  "action": "write SKILL.md.pending via sidecar",
  "scope": "Global",
  "slug": "harness-golden-1780570932593"
}
```

### Output
```json
{
  "detail": "wrote harness-golden-1780570932593/SKILL.md.pending to volume",
  "elapsed_ms": 933,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 04: `approval`

**Timestamp:** 2026-06-04T11:02:14.663804671+00:00

### Input
```json
{
  "action": "rename SKILL.md.pending -> SKILL.md",
  "scope": "global",
  "slug": "harness-golden-1780570932593"
}
```

### Output
```json
{
  "detail": "approved harness-golden-1780570932593/SKILL.md.pending → SKILL.md",
  "elapsed_ms": 994,
  "ok": true
}
```

### Infra Snapshot
```json
null
```

---

## Stage 05: `snapshot_swap`

**Timestamp:** 2026-06-04T11:02:14.793715907+00:00

### Input
```json
{
  "bug": "#156 — graph.rebuilt not published due to outbox idempotency conflict",
  "prev_graph_version": 13,
  "timeout_secs": 90
}
```

### Output
```json
{
  "detail": "graph version advanced from v13 to v15; served version confirmed",
  "elapsed_ms": 117,
  "ok": true,
  "pg_graph_version_after": 15
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:02:14.781884564+00:00",
  "pg_graph_version": 15,
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
  "redis_stream_len": 4
}
```

---

## Stage 06: `retrieval_request`

**Timestamp:** 2026-06-04T11:02:14.793967336+00:00

### Input
```json
{
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780570932593",
  "repo_path": "/tmp",
  "session_id": "golden-path-retrieval-1780570934793"
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

**Timestamp:** 2026-06-04T11:02:14.858290634+00:00

### Input
```json
{
  "elapsed_ms": 53,
  "prompt": "harness golden-path sidecar ingestion harness-golden-1780570932593",
  "session_id": "golden-path-retrieval-1780570934793"
}
```

### Output
```json
{
  "additional_context_snippet": "# Compiled Context\n\nPrompt: `harness golden-path sidecar ingestion harness-golden-1780570932593`\n\n## Skill: docker-compose-service-health\n- Description: Docker Compose healthcheck patterns for Postgres, Redis, and Qdrant — wait-for-healthy sequencing in test environments\n- Score: 0.011\n\n## Skill: rust-tokio-async-file-io\n- Description: Async file I/O patterns in Rust using tokio::fs — reading, writing, and error boundaries for async contexts\n- Score: 0.011\n\n## Skill: websocket-heartbeat-keep",
  "contains_seeded_skill": true,
  "graph_version": 15,
  "latency_ms": 49,
  "reason_code": "project_scope_resolution_failed",
  "source": "retrieval",
  "status": "degraded"
}
```

### Infra Snapshot
```json
{
  "captured_at": "2026-06-04T11:02:14.847629694+00:00",
  "pg_graph_version": 15,
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
  "redis_stream_len": 4
}
```

---

## Stage 08: `cleanup`

**Timestamp:** 2026-06-04T11:02:15.830714190+00:00

### Input
```json
{
  "scope": "global",
  "slug": "harness-golden-1780570932593"
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
