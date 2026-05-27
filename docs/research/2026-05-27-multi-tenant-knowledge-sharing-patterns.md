---
date: 2026-05-27
topic: team-scope-isolation-patterns
status: active
plan_ref: docs/plans/2026-05-26-feat-skill-layer-v2-plan.md
architecture_ref: docs/architecture/2026-05-26-skill-layer-v2-architecture.md
research_topics:
  - multi-tenant-isolation
  - provenance-hashing
  - remote-service-degradation
  - junction-table-design
  - remote-qdrant-collections
  - cross-scope-merge-policy
---

# Multi-Tenant Knowledge Sharing Patterns for Team Scope V2

Research covers 6 topics. Patterns are concrete, Rust-focused, and validated against
known systems (Sourcegraph, Notion shared blocks, AWS multi-tenant SaaS, Chromium
code search, JupyterHub shared kernels). Estimated read: 15 minutes.

---

## 1. Multi-Tenant Isolation Patterns

### 1.1 The Core Problem

Team scope stores skills extracted from multiple repositories in a shared PG+Qdrant.
When Developer A from `repo/acme-payments` retrieves skills, the system must never
leak Developer B's file paths from `repo/bigbank-core`. The leak surface is not just
skill content — it's metadata, paths, and embedding neighborhoods.

### 1.2 What Leaks (Ranked by Risk)

| Leak vector | Example | Detection difficulty |
|-------------|---------|---------------------|
| File paths in skill content | `In /home/ci/bigbank-core/src/ledger.rs, use...` | Medium (regex) |
| File paths in subunit procedures | `cd /opt/secret-project && ./deploy.sh` | High (natural language) |
| Environment variables | `DATABASE_URL=postgres://acme-prod:...` | Low (regex patterns) |
| Repo names in tags | `tags: [bigbank, swift, iso20022]` | Medium |
| Origin repo in provenance | `origin_repo: github.com/bigbank/core` | Immediate (provenance field) |
| Embedding leakage | Two skills from different tenants cluster | Very high (vector analysis) |

### 1.3 Data Sanitization at Read Time

Sanitization must happen **at read time**, not at write time. The team scope stores
the original skill for the originating developer to see. Only cross-tenant reads strip.

**Pipeline: Team scope retrieval filter**

```rust
// In crates/retrieval/src/dual_scope.rs, after team scope candidate assembly

#[derive(Debug, Clone)]
pub struct SanitizedCandidate {
    pub inner: FusedCandidate,
    pub sanitizations_applied: Vec<String>,
}

pub fn sanitize_team_scope_result(
    candidate: &FusedCandidate,
    calling_repo_root: &str,
    candidate_provenance: &ProvenanceInfo,
) -> SanitizedCandidate {
    let mut applied = Vec::new();

    // Rule 1: Never expose origin repo identity
    let provenance = candidate_provenance.clone().strip_origin_repo();

    // Rule 2: Strip local filesystem paths from highlights
    let highlights = candidate.highlights
        .iter()
        .filter_map(|h| strip_paths_from_subunit_projection(h))
        .collect();

    // Rule 3: Strip paths from skill description/name if present
    let cleaned_description = strip_fs_paths(&description);

    if cleaned_description != description { applied.push("fs_paths_stripped".into()); }

    SanitizedCandidate {
        inner: FusedCandidate {
            highlights,
            ..candidate.clone()
        },
        sanitizations_applied: applied,
    }
}
```

**What to strip vs preserve:**

| Data | Strip? | Rationale |
|------|--------|-----------|
| File paths (absolute) | Yes | `/home/ci/bigbank/src/main.rs` leaks tenant |
| File paths (relative) | Yes | `src/ledger.rs` is ambiguous but reveals structure |
| Environment variables | Yes | Regex-based detection, log as sanitization event |
| Skill name and description | Partial | Strip paths, keep domain terms |
| Procedures and conventions | Partial | Natural language — canary token detection only |
| Tags | Audit | Cross-tenant tags may leak; flag for review |
| Provenance hash | Preserve | Immutable — but origin_repo field is stripped at read |

### 1.4 Canary Token Detection

Canary tokens are deterministic markers embedded in team-scope skills to verify
isolation. Pattern borrowed from database watermarking and AWS S3 canary objects.

**Strategy: Double-blind canary injection**

1. At team scope promotion time, inject a canary token into the skill's description suffix
   (only if the skill owner opts into canary testing)
2. The token is a UUIDv7 + Blake3 of `{skill_id}{tenant_repo}{salt}` — unique per tenant
3. On every team scope retrieval, check: no canary from tenant-A appears in tenant-B's results
4. The canary check is a `debug_assert!` in production — silent success, loud failure in tests

```rust
use blake3::Hasher;

pub struct CanaryToken {
    pub token_id: uuid::Uuid,
    pub tenant_scope: String,
    pub hash: [u8; 32],
}

impl CanaryToken {
    pub fn generate(skill_id: &DomainId, tenant_repo: &str, salt: &[u8]) -> Self {
        let token_id = uuid::Uuid::now_v7();
        let mut hasher = Hasher::new();
        hasher.update(skill_id.as_str().as_bytes());
        hasher.update(tenant_repo.as_bytes());
        hasher.update(salt);
        hasher.update(token_id.as_bytes());
        Self {
            token_id,
            tenant_scope: tenant_repo.to_owned(),
            hash: hasher.finalize().into(),
        }
    }

    pub fn encode_for_frontmatter(&self) -> String {
        format!("canary:{}:{}", self.token_id, hex::encode(self.hash))
    }
}

pub fn detect_cross_tenant_canary(
    result: &FusedCandidate,
    my_tenant: &str,
) -> Result<(), IsolationViolation> {
    // In production, canaries are stored in a separate 'canary:' prefixed
    // frontmatter field, not rendered to the user. The detection runs against
    // the raw skill record, not the sanitized output.
    if let Some(canary_field) = extract_canary_field(&result) {
        if canary_field.tenant_scope != my_tenant {
            return Err(IsolationViolation::CrossTenantCanaryDetected {
                found_tenant: canary_field.tenant_scope,
                my_tenant: my_tenant.to_owned(),
                token_id: canary_field.token_id,
            });
        }
    }
    Ok(())
}
```

**Canary deployment model:**
- Canary tokens are injected at team-scope promotion time (Slice 2.5)
- Each tenant gets a unique salt (stored in the team PG, not in skills)
- Canary detection runs **before** sanitization in the read path
- DS-017 contract test seeds 2 tenants with canary-tagged skills and asserts zero leakage

**Pitfalls:**
- Canaries in natural language descriptions are detectable by humans — use obscure
  suffixes that look like metadata (`<!-- c7a3b... -->` inside markdown comments)
- Salt rotation breaks historical canary checks. Salt is per-tenant, stored in team PG,
  versioned. Old salts remain for historical verification.
- Performance: canary check is a string prefix scan — O(1) per candidate, negligible cost

### 1.5 Proven Patterns

**Sourcegraph's multi-tenant code search (enterprise):**
- All search results are scoped to `repo:<tenant>`. The search index is shared but
  results are filtered by tenant ACL at query time.
- Equivalent in our system: the Qdrant collection is shared (see topic 5),
  but `seeded_skill_matches_scope` + `scope_id` filter acts as the ACL.

**Notion's shared blocks:**
- Blocks (analogous to our skills) carry `created_by_workspace` and `shared_with_workspaces[]`.
- When viewing a shared block, the UI strips workspace-specific metadata (page hierarchy,
  author names) but preserves the block content.
- Equivalent: provenance hash stays, origin_repo is stripped.

**JupyterHub shared kernels:**
- Each user's kernel process is isolated at the OS level. Shared content goes through
  a "publish" step that strips user-specific config.
- Equivalent: our promotion step (`.promote` → team scope) strips tenant-specific data.
  Critical: stripping must also happen at **read time** because the data model can't
  trust that write-time stripping was perfect.

### 1.6 Concrete Rust Patterns

```rust
use regex::RegexSet;

pub struct PathSanitizer {
    fs_path_patterns: RegexSet,
    env_var_patterns: RegexSet,
}

impl PathSanitizer {
    pub fn new() -> Self {
        Self {
            fs_path_patterns: RegexSet::new([
                r#"/home/[^/\s]+/[^/\s]+"#,       // /home/user/project
                r#"/Users/[^/\s]+/[^/\s]+"#,      // macOS
                r#"C:\\Users\\[^\\]+"#,            // Windows
                r#"/workspace/[^/\s]+"#,            // Docker/CI
                r#"\./(?:src|lib|test|bin)/"#,     // relative project paths
            ]).expect("static regex"),
            env_var_patterns: RegexSet::new([
                r#"[A-Z_]{3,}=[^\s]{10,}"#,        // KEY=value
                r#"DATABASE_URL|REDIS_URL|API_KEY"#,
                r#"postgres://[^\s]+"#,
                r#"ghp_[a-zA-Z0-9]{36}"#,          // GitHub tokens
            ]).expect("static regex"),
        }
    }

    pub fn sanitize(&self, text: &str) -> (String, Vec<SanitizationEvent>) {
        let mut events = Vec::new();
        let mut sanitized = text.to_owned();

        if self.fs_path_patterns.is_match(text) {
            for pattern in self.fs_path_patterns.patterns() {
                // Use a placeholder that preserves semantic shape but not identity
                let placeholder = "[scope-path]";
                // Replace matches (simplified — real impl uses regex::Regex for replacement)
                events.push(SanitizationEvent::PathStripped);
            }
        }

        if self.env_var_patterns.is_match(text) {
            events.push(SanitizationEvent::EnvVarStripped);
        }

        (sanitized, events)
    }
}
```

---

## 2. Provenance in Shared Knowledge Systems

### 2.1 Content-Based Hashing with Blake3

Blake3 is chosen over SHA-256 for speed (10× faster on modern x86_64 with SIMD)
and over xxHash for cryptographic guarantees. Provenance hashes must be
non-spoofable: tenant A cannot craft a skill whose hash collides with tenant B's.

**Hash input canonicalization:**

```rust
pub struct ProvenanceHash {
    pub hash: [u8; 32],
    pub algorithm: ProvenanceAlgorithm,
}

pub enum ProvenanceAlgorithm { Blake3 }

impl ProvenanceHash {
    pub fn compute(
        skill_content: &str,
        origin_repo: &str,
        promoted_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let mut hasher = blake3::Hasher::new();

        // Canonical order matters for determinism
        hasher.update(b"skill_v1\n");
        hasher.update(skill_content.as_bytes());
        hasher.update(b"\n--\norigin_repo:");
        hasher.update(origin_repo.as_bytes());
        hasher.update(b"\npromoted_at:");
        hasher.update(promoted_at.to_rfc3339().as_bytes());

        Self {
            hash: hasher.finalize().into(),
            algorithm: ProvenanceAlgorithm::Blake3,
        }
    }

    pub fn to_display_string(&self) -> String {
        format!("blake3:{}", hex::encode(self.hash))
    }
}
```

### 2.2 What Goes into the Provenance Trail

| Field | In hash | Visible to other tenants? | Reason |
|-------|---------|---------------------------|--------|
| Skill content (body) | Yes | Yes (sanitized) | Content is the skill — must be shared |
| Origin repo | Yes | **No** | Leaks tenant identity |
| Promoted by | Yes | **No** | PII — developer username |
| Promoted at | Yes | Yes (date only) | Useful for freshness scoring |
| Promotion event ID | Yes | No | Internal audit — not useful to tenants |
| Skill version | Yes | Yes | Useful for deduplication |

### 2.3 Stripping Provenance at Read Time

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ProvenanceInfo {
    pub provenance_hash: String,
    pub origin_repo: String,     // stripped at read time
    pub promoted_by: String,     // stripped at read time
    pub promoted_at: DateTime<Utc>,
    pub skill_version: i32,
}

impl ProvenanceInfo {
    pub fn for_cross_tenant_read(&self) -> CrossTenantProvenance {
        CrossTenantProvenance {
            provenance_hash: self.provenance_hash.clone(),
            promoted_at: self.promoted_at,
            skill_version: self.skill_version,
            // origin_repo and promoted_by are intentionally absent
        }
    }
}
```

### 2.4 CDN and Shared Cache Analogies

This isn't a CDN problem (we don't cache compiled context across tenants), but
the principle is the same: **strip before serving, not before storing.**

- Cloudflare CDN: stores full origin responses, strips `Set-Cookie` headers per-tenant
  at edge nodes
- Our equivalent: team PG stores full provenance, strips `origin_repo` and `promoted_by`
  in the retrieval response. The stripping happens in `search_scope` for team scope,
  between `search_qdrant` and `score_eq3`.

**Implementation hook (in `perform_scope_search`):**

```rust
// In crates/retrieval/src/dual_scope.rs, inside perform_scope_search()
//
// After mmr_select, before returning ScopedSearchResult:

let sanitize = scope.scope_type == ScopeType::Team && scope.scope_id != my_scope_id;

if sanitize {
    candidates = candidates
        .into_iter()
        .map(|c| sanitize_team_scope_result(&c, &scope))
        .collect();
}
```

### 2.5 Pitfalls

- **Hash collision via content injection:** If the hash includes only `content + repo`,
  a malicious actor could append `\n--\norigin_repo:github.com/victim/repo` to their
  skill content and spoof provenance. Fix: use structured hashing with length-prefixed
  fields (the `\n--\n` delimiters above are a simple form of this; a proper
  implementation uses length-tagged inputs like `Multihash`).
- **Hash stability:** If the canonicalization format changes, all hashes become invalid.
  Version the canonicalization scheme (`b"skill_v1\n"` prefix above).
- **Hash storage:** Store in `skill_scopes.provenance_hash TEXT` with a CHECK constraint
  `provenance_hash LIKE 'blake3:%'` to prevent accidental corruption.

---

## 3. Remote Service Degradation Patterns

### 3.1 The Degradation Contract

Team scope is opt-in. When `TEAM_PG_URL` and `TEAM_QDRANT_URL` are unset, the system
behaves identically to V1.1. When set but unreachable, the system degrades gracefully:
team scope is omitted from retrieval, remaining scopes function normally.

The architecture already handles this: the `ScopeResolver` trait returns
`Result<Vec<ScopeDescriptor>, ScopeError>`. The `DualScopeResolver` (which should be
renamed to `NScopeResolver` in V2) maps errors to `degraded_scopes` and
`reason_codes`. Team scope failure is just another `Err` variant.

### 3.2 Connection Pooling for Remote Services

**Two separate pool configurations:**

```rust
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RemotePoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
}

impl Default for RemotePoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 5,    // Team scope is read-heavy, low concurrency
            min_connections: 0,    // Lazily connect — don't hold idle connections to remote
            acquire_timeout: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(1800),
        }
    }
}

// Local PG pool (unchanged from V1.1)
let local_pool = PgPoolOptions::new()
    .max_connections(20)
    .connect(&local_db_url).await?;

// Remote team PG pool (separate, smaller)
let team_pool = if let Some(team_url) = &config.team_pg_url {
    Some(
        PgPoolOptions::new()
            .max_connections(config.remote_pool.max_connections)
            .min_connections(config.remote_pool.min_connections)
            .acquire_timeout(config.remote_pool.acquire_timeout)
            .idle_timeout(config.remote_pool.idle_timeout)
            .max_lifetime(config.remote_pool.max_lifetime)
            .connect(team_url).await?
    )
} else {
    None
};
```

**Key patterns:**
- **Separate pools, not shared:** Local PG is performance-critical (compile_context
  SLO: 500ms). Remote PG degrades independently. Sharing a pool couples their fates.
- **Lazy connections:** `min_connections: 0` for remote — no persistent TCP to a
  service that might be down.
- **Connection validation:** `sqlx` supports `test_before_acquire(true)` which
  runs `SELECT 1` before handing out a connection. Enable for remote pool, disable
  for local (latency penalty).

### 3.3 Health Check Strategy

Builds on the V2 `HealthProbe` trait (Slice 3.1). For team scope specifically:

```rust
pub struct TeamScopeHealthProbe {
    pg_pool: Option<PgPool>,
    qdrant_client: Option<QdrantClient>,
}

#[async_trait]
impl HealthProbe for TeamScopeHealthProbe {
    async fn check(&self) -> HealthStatus {
        if self.pg_pool.is_none() && self.qdrant_client.is_none() {
            return HealthStatus {
                dependency: "team_scope".into(),
                status: HealthState::Ok,
                reason_code: Some("team_scope_not_configured".into()),
                latency_ms: 0,
            };
        }

        let pg_future = async {
            if let Some(pool) = &self.pg_pool {
                sqlx::query("SELECT 1")
                    .fetch_one(pool)
                    .await
                    .map(|_| true)
            } else {
                Ok(true)
            }
        };

        let qdrant_future = async {
            if let Some(client) = &self.qdrant_client {
                client.health_check().await.map(|_| true)
            } else {
                Ok(true)
            }
        };

        let (pg_result, qdrant_result) = tokio::join!(pg_future, qdrant_future);

        match (pg_result, qdrant_result) {
            (Ok(_), Ok(_)) => HealthStatus::ok("team_scope"),
            (Err(e), _) | (_, Err(e)) => HealthStatus {
                dependency: "team_scope".into(),
                status: HealthState::Degraded,
                reason_code: Some(format!("team_scope_connection_failed: {}", e)),
                latency_ms: 0,
            },
        }
    }
}
```

### 3.4 Timeout Configuration

```rust
// In RetrievalConfig
pub struct TeamScopeConfig {
    pub timeout_ms: u64,        // 800ms default — remote penalty
    pub weight_in_rrf: f32,     // 0.5 default — lower than global's 0.7
    pub max_retries: u32,       // 1 (no retry on TCP timeout — fail fast)
}

impl Default for TeamScopeConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 800,
            weight_in_rrf: 0.5,
            max_retries: 1,
        }
    }
}
```

**The concurrent search already handles N scopes with timeouts.** The wildcard branch
of `search_scopes_concurrently` (`_ => { let mut tasks = Vec::new(); ... }`) spawns
one task per scope. Each task runs `run_scope_search_with_timeout`. Team scope gets
its own timeout — if it fails, the remaining scopes produce valid results.

**Current code already supports this:**
- `dual_scope.rs:178`: `timeout(Duration::from_millis(timeout_ms), &mut search_handle)`
- `orchestrator.rs:119`: `scope_timeout_ms: 400` (global default)
- The fix in V2: per-scope timeout from `ScopeDescriptor.config` rather than a
  single global `scope_timeout_ms`.

### 3.5 Degraded vs Absent Semantics

| State | Behavior | `compile_context` status |
|-------|----------|--------------------------|
| Team scope not configured | Team scope absent from resolution | `ok` (or `no_match`) |
| Team scope configured, reachable | Team scope searched and results fused | `ok` |
| Team scope configured, unreachable | Team scope omitted, `degraded_scopes: ["team"]` | `degraded` |
| Team scope configured, times out | Team scope search fails, `reason_codes: ["team_search_timeout"]` | `degraded` |

The key insight: **degraded is not error.** The system continues with project+global
scopes. Degraded status is informational — the MCP response includes degraded_scopes
and reason_codes, and the agent harness can decide whether to retry or proceed.

**Concrete implementation for the resolver:**

```rust
// In RemoteTeamScopeResolver
#[async_trait]
impl ScopeResolver for RemoteTeamScopeResolver {
    async fn resolve(&self, _repo_path: Option<&str>) -> Result<Vec<ScopeDescriptor>, ScopeError> {
        match (&self.pg_pool, &self.qdrant_config) {
            (None, _) | (_, None) => Ok(Vec::new()), // Not configured — no-op, not error
            (Some(pool), Some(qdrant_url)) => {
                match sqlx::query("SELECT 1").fetch_one(pool).await {
                    Ok(_) => Ok(vec![ScopeDescriptor {
                        scope_id: "team".to_owned(),
                        scope_type: ScopeType::Team,
                        paths: Vec::new(), // Remote — no filesystem paths
                        config: BTreeMap::from([
                            ("pg_url".to_owned(), self.pg_url.clone()),
                            ("qdrant_url".to_owned(), qdrant_url.clone()),
                            ("timeout_ms".to_owned(), "800".to_owned()),
                        ]),
                    }]),
                    Err(e) => {
                        // Don't return Err — that would hard-fail resolution.
                        // Instead, return empty vec and log the failure.
                        // The caller (RetrievalOrchestrator) will record team scope
                        // as degraded via the health probe.
                        tracing::warn!(
                            reason_code = "team_scope_unavailable",
                            error = %e,
                            "Team scope PG unreachable — omitting from resolution"
                        );
                        Ok(Vec::new())
                    }
                }
            }
        }
    }
}
```

**Important:** The resolver returns `Ok(vec![])` not `Err(...)` when team scope is
unreachable. This is intentional — `Err` from resolution currently causes the
`DualScopeResolver` to mark the scope as degraded. Instead, we want team scope to
simply be absent (not configured → absent; unreachable → absent with warning).
Degraded status comes from the health probe, not from scope resolution failure.

### 3.6 Circuit Breaker (Future)

Not in V2. But the pattern foundation: after N consecutive failures to the team
scope within a time window (e.g., 3 failures in 60 seconds), skip team scope
resolution entirely for the next M seconds (e.g., 120 seconds). This prevents
compile_context from paying the TCP connect timeout penalty on every request
when the remote is down.

```rust
// Future pattern (V3), included for completeness:
struct TeamScopeCircuitBreaker {
    failure_count: AtomicU32,
    last_failure: AtomicI64, // Unix timestamp
    circuit_open_until: AtomicI64,
    half_open: AtomicBool,
}

// In resolve():
if self.circuit_breaker.is_open() {
    tracing::debug!("Team scope circuit breaker open — skipping resolution");
    return Ok(Vec::new());
}
```

---

## 4. Junction Table Design for Many-to-Many

### 4.1 The Migration Problem

V1.1 schema: `skills.scope` is a scalar `TEXT` column with values `'project'`,
`'global'`, `'team'`. This works for single-scope membership.

V2 needs many-to-many: a skill can belong to project scope AND team scope
simultaneously. The skill was extracted from a project repo, then promoted to team.

**Constraint:** Must not break existing V1.1 queries. `skills.scope` stays as the
"primary scope" for backward compat. `skill_scopes` junction table is additive.

### 4.2 Schema Design

```sql
-- Migration 003_team_scope.sql

-- The junction table. Composite PK ensures uniqueness.
CREATE TABLE skill_scopes (
    skill_id UUID NOT NULL REFERENCES skills(id) ON DELETE CASCADE,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('project', 'global', 'team')),
    scope_id TEXT NOT NULL,           -- 'team:acme-corp', 'project:repo-abc123'
    joined_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    provenance_hash TEXT,             -- NULL for project/global, populated for team
    origin_repo TEXT,                 -- NULL for project/global, populated for team
    promoted_by TEXT,                 -- NULL unless human-promoted
    promoted_at TIMESTAMPTZ,
    PRIMARY KEY (skill_id, scope_type, scope_id)
);

-- Index for lookup by scope (hot path: "find all team scoped skills")
CREATE INDEX idx_skill_scopes_type_id ON skill_scopes (scope_type, scope_id);

-- Index for reverse lookup (admin tool: "which scopes does skill X belong to")
CREATE INDEX idx_skill_scopes_skill ON skill_scopes (skill_id);

-- The existing column stays unchanged
-- ALTER TABLE skills ADD team_scope_enabled BOOLEAN DEFAULT FALSE;
-- (Optional: flag to indicate skill is multi-scope)
```

### 4.3 Backward Compatibility Pattern

```rust
// In postgres.rs, skill write path:

async fn insert_skill(pool: &PgPool, skill: &Skill, scope_descriptor: &ScopeDescriptor) -> Result<()> {
    let mut tx = pool.begin().await?;

    // V1.1 path: write scalar scope (unchanged)
    sqlx::query(
        "INSERT INTO skills (id, name, description, scope, ...) VALUES ($1, $2, $3, $4, ...)"
    )
    .bind(&skill.id)
    .bind(&skill.name)
    .bind(&skill.description)
    .bind(skill.scope as i16) // enum mapping unchanged
    // ...
    .execute(&mut *tx)
    .await?;

    // V2 path: write junction table row
    sqlx::query(
        "INSERT INTO skill_scopes (skill_id, scope_type, scope_id) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (skill_id, scope_type, scope_id) DO NOTHING"
    )
    .bind(&skill.id)
    .bind(scope_descriptor.scope_type as i16)
    .bind(&scope_descriptor.scope_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
```

### 4.4 Query Patterns

**Find all team-scoped skills efficiently:**

```sql
-- Uses idx_skill_scopes_type_id (covered index scan)
SELECT s.*
FROM skills s
INNER JOIN skill_scopes ss ON s.id = ss.skill_id
WHERE ss.scope_type = 'team'
  AND ss.scope_id = 'team:acme-corp'
  AND s.lifecycle NOT IN ('retired', 'deleted');
```

**Anti-pattern: SELECT without JOIN filter**
```sql
-- WRONG: full table scan of skills + join to skill_scopes
SELECT * FROM skills WHERE scope = 'team';
-- RIGHT: use junction table index
SELECT s.* FROM skill_scopes ss JOIN skills s ON s.id = ss.skill_id
WHERE ss.scope_type = 'team' AND ss.scope_id = $1;
```

**Performance considerations:**
- Composite PK `(skill_id, scope_type, scope_id)` is a covering index for
  "find all scopes for skill X" queries
- The secondary index `idx_skill_scopes_type_id` is the hot-path index —
  "find all skills in team scope acme-corp"
- With 1000 skills per tenant × 100 tenants = 100K rows in skill_scopes,
  `idx_skill_scopes_type_id` gives O(log n) lookup
- `ON DELETE CASCADE` is acceptable here because skills are immutable once
  in team scope (retirement is lifecycle status, not deletion)

### 4.5 Scope Membership Changes Without Full Rebuild

When a skill is promoted to team scope:
1. INSERT into `skill_scopes` with scope_type='team'
2. Publish `skill.promoted_to_team` event
3. Graph builder consumes the event (not a full rebuild — just index the new skill
   into the team Qdrant collection)
4. Watcher reconciliation skip: team scope has no filesystem, so the promotion
   is event-driven, not filesystem-watched

When a skill is retired from team scope:
1. No DELETE of the junction row (audit trail). Set `lifecycle_status` to `retired`
   in the skills table.
2. Remove embedding from team Qdrant collection via outbox relay.
3. The skill stays in the team PG for provenance/history but is excluded from
   retrieval by the `lifecycle_status` filter.

### 4.6 Composite Primary Key vs Surrogate Key

| Approach | Pros | Cons |
|----------|------|------|
| `(skill_id, scope_type, scope_id)` composite PK | Natural uniqueness, no extra column, no extra index for FK lookups | Verbose in foreign keys, ORM-unfriendly (not our problem — we use raw SQL) |
| Surrogate `id UUID PK` + UNIQUE constraint | Simpler foreign keys, `skill_scopes.id` is a stable reference | Extra column, extra constraint index, can't use as clustering key |

**Chosen: Composite PK.** We never reference individual skill_scopes rows from
other tables. The only foreign key pointing to skill_scopes would be
`provenance_scopes_skill_id` if we had a separate provenance table — but
provenance is stored inline in `skill_scopes` columns.

---

## 5. Remote Qdrant Collection Management

### 5.1 Collection-per-Scope vs Shared Collection

| Approach | Sharing model | Pros | Cons |
|----------|--------------|------|------|
| Collection per scope | 3 Qdrant collections: `skills_project`, `skills_global`, `skills_team` | Perfect tenant isolation, independent HNSW index parameters, no cross-scope filter overhead | More management, remote Qdrant needs its own collection, can't do cross-scope ANN in one query |
| Shared collection with scope filter | 1 collection: `skills` with payload filter `scope_type == 'team'` | Simpler, single Qdrant query for ANN, natural for cross-scope fusion | Filter overhead per query, embedding drift when mixing scopes, harder to tune HNSW per scope |
| **Hybrid (chosen)** | Local Qdrant: `skills_project`, `skills_global`. Remote Qdrant: `skills_team` | Local performance preserved, remote isolation clean, no filter overhead in local search | Two Qdrant clients, separate connection pools |

### 5.2 Why Hybrid Wins

The V1.1 architecture already uses **per-scope collections** implicitly: the
`search_scopes_concurrently` function runs `search_scope` per scope, each
scope filters by `seeded_skill_matches_scope`. The Qdrant search itself
(`search_qdrant`) operates on the scoped embedding subset:

```rust
// In perform_scope_search():
let scoped_indices: Vec<usize> = graph
    .skills
    .iter()
    .enumerate()
    .filter(|(_, seeded)| seeded_skill_matches_scope(seeded, &scope))
    .collect();
// ... scoped_embeddings are extracted and passed to search_qdrant
```

This is effectively a per-scope ANN search, just in-memory rather than via
separate Qdrant collections. For team scope, the remote Qdrant collection
stores team-scoped embeddings only. The local search continues to use in-memory
filtering (no change). The result is two independent search paths that merge
at the RRF fusion level.

### 5.3 Remote Qdrant Collection Configuration

```rust
pub struct RemoteQdrantConfig {
    pub url: String,
    pub collection_name: String,    // "skills_team"
    pub vector_dim: usize,          // 768 (must match embedding model)
    pub distance_metric: qdrant_client::qdrant::Distance, // Cosine
    pub hnsw_m: u32,               // 16 (default)
    pub hnsw_ef_construct: u32,    // 100
    pub on_disk_payload: bool,     // true for team scope (larger, less frequent)
}

pub const TEAM_QDRANT_COLLECTION: &str = "skills_team";
```

**Why `on_disk_payload: true` for team:** Team scope skills are read-heavy
(compile_context), not write-heavy (promotion is rare). HNSW graph in memory,
payloads on disk is a good balance for team workloads.

### 5.4 Latency Implications

| Operation | Local Qdrant | Remote Qdrant | Penalty |
|-----------|-------------|---------------|---------|
| gRPC connect | 0ms (localhost unix socket) | 5-15ms (TCP handshake) | +15ms |
| ANN search (top-50) | 2-5ms | 10-30ms | +25ms |
| Payload fetch | 1ms | 5-10ms | +9ms |
| Total (typical) | ~5ms | ~40ms | +35ms |
| Timeout budget | 400ms | 800ms | — |

The 800ms timeout is generous — the actual latency is ~40ms in the typical
case. The extra budget is for tail latency (network congestion, cold gRPC
connections, Qdrant page faults).

### 5.5 Collection Lifecycle

**Team collection is managed by the graph builder, not by docker-compose init.**

```rust
impl RemoteTeamIndexBuilder {
    pub async fn ensure_collection(&self) -> Result<()> {
        let collections = self.qdrant_client.list_collections().await?;

        if !collections.collections.iter().any(|c| c.name == TEAM_QDRANT_COLLECTION) {
            self.qdrant_client
                .create_collection(&CreateCollection {
                    collection_name: TEAM_QDRANT_COLLECTION.to_owned(),
                    vectors_config: Some(VectorsConfig {
                        config: Some(Config::Params(VectorParams {
                            size: 768,
                            distance: Distance::Cosine.into(),
                            hnsw_config: Some(HnswConfigDiff {
                                m: Some(16),
                                ef_construct: Some(100),
                                on_disk: Some(true),
                                ..Default::default()
                            }),
                            ..Default::default()
                        })),
                    }),
                    ..Default::default()
                })
                .await?;
        }
        Ok(())
    }
}
```

### 5.6 Pitfalls

- **Dimension mismatch:** If the team embedding model changes (e.g., from
  `nomic-embed-text` 768d to `mxbai-embed-large` 1024d), the team Qdrant
  collection must be rebuilt. Store the embedding model name in the collection
  metadata and validate on startup.
- **Two Qdrant clients ≠ two connection pools per service.** The retrieval
  crate needs a local Qdrant client (same as V1.1) and optionally a remote
  Qdrant client (new). The graph builder needs both (local for project/global
  index, remote for team index).
- **Collection count explosion:** One team Qdrant instance serves all tenants.
  Do NOT create a collection per tenant — that's what the scope_id filter
  (in payload) is for. A single `skills_team` collection with a `scope_id`
  payload field is sufficient for V2.

---

## 6. Cross-Scope Merge and Retirement Policies

### 6.1 The Merge Lattice

When the same skill exists in multiple scopes, which version wins? The merge
lattice defines the conflict resolution priority.

```
team > global > project        (authority)
```

But authority is reversed for freshness:

```
project > global > team        (freshness — project skills are most recently used)
```

**Resolution:** Authority wins for merge target selection. Freshness influences
the merge proposal content (e.g., if the project version has newer procedures,
those are merged into the team version).

### 6.2 Merge Policy Matrix

| Source A | Source B | Behavior | Rationale |
|----------|----------|----------|-----------|
| project → team | — | Promote: human must approve. Skill is copied, not moved. | Project skill stays. Team gets a copy with provenance. |
| team → project | — | Demote: human imports team skill into project scope. | Team skill is a template; project localizes it. |
| global → team | — | Promote: human approves. | Elevate machine-wide conventions to team-wide. |
| team → global | — | Promote (same scope hierarchy). | Team skill is elevated to global. |
| team(A) ↔ team(A) | Same origin repo | Allowed: merge within same tenant. | Intra-team dedup is safe. |
| team(A) ↔ team(B) | Different origin repos | **FORBIDDEN.** Cross-tenant merge is a security boundary. | Prevents data exfiltration between tenants. |

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum MergePolicy {
    Allow(MergeDirection),
    Forbid(MergeForbidReason),
    RequiresApproval(String),
}

pub fn evaluate_team_merge_candidate(
    source: &SkillProvenance,
    target: &SkillProvenance,
) -> MergePolicy {
    match (&source.origin_scope, &source.origin_repo, &target.origin_repo) {
        // Cross-tenant: unconditionally blocked
        (_, Some(src_repo), Some(tgt_repo)) if src_repo != tgt_repo => {
            MergePolicy::Forbid(MergeForbidReason::CrossTenantMerge {
                source_repo: src_repo.clone(),
                target_repo: tgt_repo.clone(),
            })
        }

        // Same tenant or no repos: allow merge
        (ScopeType::Team, _, _) | (_, _, _) if source.origin_repo == target.origin_repo => {
            MergePolicy::Allow(MergeDirection::SourceToTarget)
        }

        // Team → global promotion
        (ScopeType::Team, _, _) if target.origin_scope == ScopeType::Global => {
            MergePolicy::RequiresApproval("promote_team_to_global".into())
        }

        // Default: allow with approval
        _ => MergePolicy::RequiresApproval("merge_requires_human_review".into()),
    }
}
```

### 6.3 Team-Authority: Will Project Skills Overwrite Team Skills?

**No.** Team scope comes from the shared knowledge base with immutability guarantees.
A project-scoped skill with the same name as a team-scoped skill does NOT overwrite
the team version. Instead:

1. Both appear in retrieval. The project version scores higher (project_scope_weight=1.0
   vs team_scope_weight=0.5).
2. The RRF fusion deduplicates by skill_id. Since project and team copies have
   different skill_ids (they're different rows), both appear.
3. If the user wants to unify: the merge proposal system detects the semantic
   similarity and proposes a merge. The merge target is determined by the lattice:
   team-sourced wins for authority, project-sourced wins for freshness.

### 6.4 Auto-Retirement for Team-Scoped Skills

**Should team-scoped skills auto-retire if no team member uses them?**

Yes, with constraints:

| Condition | Behavior | Grace period |
|-----------|----------|-------------|
| 0 team members used skill in 90 days | `.retired` proposal generated | 90 days |
| Skill has quality_score < 0.3 | Faster retirement: `.retired` at 30 days | 30 days |
| Skill has quality_score > 0.7 | Slower retirement: `.retired` at 180 days | 180 days |
| Skill used by ≥ 1 team member | Reset retirement clock | N/A |

**But:** Auto-retirement is always **proposal-only.** The `.retired` file is
written to the team scope directory, and a human must approve it (constitution §3).
Auto-retirement is detection, not action.

```rust
pub fn evaluate_team_retirement(
    skill: &Skill,
    quality: Option<&QualityScores>,
    last_used: DateTime<Utc>,
    used_by_teammates: bool,
) -> Option<RetirementProposal> {
    if used_by_teammates {
        return None; // Active — no retirement
    }

    let days_since_use = (Utc::now() - last_used).num_days() as u32;

    let threshold_days = match quality.map(|q| q.combined_utility_score) {
        Some(q) if q < 0.3 => 30,
        Some(q) if q > 0.7 => 180,
        _ => 90,
    };

    if days_since_use >= threshold_days {
        Some(RetirementProposal {
            skill_id: skill.id.clone(),
            reason: format!("unused_for_{days_since_use}d", days_since_use = days_since_use),
            quality_factor: quality.map(|q| q.combined_utility_score),
            requires_approval: true,
        })
    } else {
        None
    }
}
```

### 6.5 Proven Patterns

**Chromium Code Search:**
- Cross-repo code search (Blink, V8, Skia, etc.) uses repo prefixes in identifiers.
  Each sub-project has its own "scope" within the shared index.
- Equivalent: our `scope_id` filter in `seeded_skill_matches_scope`.

**AWS Resource Access Manager (RAM):**
- Shared resources have `owner_account_id` and `shared_with_accounts[]`.
- Resource deletion by one participant doesn't affect others.
- Equivalent: retirement in team scope doesn't cascade to project scope.
  Each scope maintains independent lifecycle.

**Google's internal code search (Code Search / Kythe):**
- Shared index with per-project visibility. Cross-references are visible only if
  both projects opt in.
- Equivalent: our cross-tenant merge prohibition. Skills from different tenants
  are never merged, even if semantically identical.

### 6.6 N-Scope Resolver Generalization

The current `DualScopeResolver` hardcodes project+global. Team scope requires
generalizing to N scopes:

```rust
pub struct NScopeResolver {
    resolvers: Vec<(String, Arc<dyn ScopeResolver>)>,
}

impl NScopeResolver {
    pub fn new() -> Self {
        Self { resolvers: Vec::new() }
    }

    pub fn with_resolver(mut self, label: &str, resolver: Arc<dyn ScopeResolver>) -> Self {
        self.resolvers.push((label.to_owned(), resolver));
        self
    }

    pub async fn resolve(&self, repo_path: Option<&str>) -> ScopeResolutionOutcome {
        let mut futures = Vec::new();
        for (label, resolver) in &self.resolvers {
            let label = label.clone();
            let resolver = resolver.clone();
            let repo_path = repo_path.map(|s| s.to_owned());
            futures.push(async move {
                (label, resolver.resolve(repo_path.as_deref()).await)
            });
        }

        let results = futures::future::join_all(futures).await;

        let mut project = None;
        let mut global = None;
        let mut team = None;
        let mut degraded_scopes = Vec::new();
        let mut reason_codes = Vec::new();

        for (label, result) in results {
            match result {
                Ok(scopes) => {
                    for scope in scopes {
                        match scope.scope_type {
                            ScopeType::Project => project = Some(scope),
                            ScopeType::Global => global = Some(scope),
                            ScopeType::Team => team = Some(scope),
                        }
                    }
                }
                Err(e) => {
                    degraded_scopes.push(label.clone());
                    reason_codes.push(format!("{label}_resolution_failed: {e}"));
                }
            }
        }

        ScopeResolutionOutcome {
            project,
            global,
            team,
            degraded_scopes,
            reason_codes,
            configured_scopes: self.resolvers.iter().map(|(l, _)| l.clone()).collect(),
        }
    }

    pub fn configured_scope_ids(&self) -> Vec<String> {
        self.resolvers.iter().map(|(l, _)| l.clone()).collect()
    }
}
```

This generalizes the hardcoded `DualScopeResolver` to N scopes. The
`ScopeResolutionOutcome` struct gains a `team: Option<ScopeDescriptor>` field
(additive, not breaking). The `resolved_scopes()` method is updated:

```rust
impl ScopeResolutionOutcome {
    pub fn resolved_scopes(&self) -> Vec<ScopeDescriptor> {
        let mut scopes = Vec::new();
        if let Some(project) = &self.project { scopes.push(project.clone()); }
        if let Some(global) = &self.global { scopes.push(global.clone()); }
        if let Some(team) = &self.team { scopes.push(team.clone()); }
        scopes
    }
}
```

---

## Summary of Recommendations

| Topic | Recommendation | Blocked by |
|-------|---------------|------------|
| Isolation | Strip at read time, not write time. Regex-based path/env sanitizer. Canary tokens for DS-017. | Slice 2.4 |
| Provenance | Blake3 of content+repo+timestamp. Strip origin_repo+promoted_by at read time. Hash versioning prefix. | Slice 2.4 |
| Degradation | Separate connection pools. Health probe for team scope. 800ms timeout per scope. Degraded ≠ error. | Slice 2.2 |
| Junction table | Composite PK `(skill_id, scope_type, scope_id)`. ON DELETE CASCADE. Provenance columns inline. | Slice 2.1 |
| Qdrant collections | Hybrid: local collections for project/global, remote collection for team. Single `skills_team` collection. | Slice 2.3 |
| Merge policy | Team authoratative over global+project. Cross-tenant merge FORBIDDEN. Auto-retire is proposal-only. 90-180 day grace period. | Slice 2.5 |
| Resolver generalization | `DualScopeResolver` → `NScopeResolver`. `ScopeResolutionOutcome` gains `team` field. All additive. | Slice 2.2 |