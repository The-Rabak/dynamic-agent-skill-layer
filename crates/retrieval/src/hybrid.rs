/// Trait and types for the `QdrantHybrid` read-path candidate source.
///
/// Defined in `retrieval` (not `infrastructure`) so the retrieval crate does not
/// gain a dependency on the Qdrant HTTP adapter. The trait is implemented in
/// `mcp-server`, which can depend on both `retrieval` and `infrastructure`.
///
/// # CQRS contract break for `qdrant_hybrid`
///
/// Under `SnapshotDense` and `SnapshotHybrid`, Qdrant is a pure write-side store
/// (Option A, ADR-0001): retrieval serves entirely from the in-memory
/// `RetrievalSnapshot` and a Qdrant outage cannot degrade `compile_context`.
///
/// `QdrantHybrid` intentionally breaks this contract: it queries Qdrant at
/// request time to obtain dense+sparse fused candidate rankings. This means:
///   - Qdrant down ⟹ `QdrantHybrid` retrieval fails loud (explicit degraded
///     marker), NOT silent fallback to dense.
///   - The T08 ADR will formally document this CQRS break and the operational
///     implications (runbook, degradation boundary).
///
/// Do NOT add a silent dense fallback here — mislabeling the arm violates
/// the no-fakes mandate (#243).
use async_trait::async_trait;

/// Identity and Qdrant-fused relevance score for a single hybrid query hit.
///
/// The `score` is the reciprocal-rank-fusion (RRF) value returned by Qdrant's
/// `fusion: rrf` step. It is NOT a raw cosine similarity; it is comparable only
/// within a single query's result set. `score_eq3` in `dual_scope.rs` recomputes
/// the authoritative retrieval score from the full snapshot skill.
#[derive(Debug, Clone)]
pub struct HybridCandidate {
    /// Stable skill ID (`skills.id` in Postgres), used to join against the
    /// in-memory `RetrievalSnapshot`. Must match exactly what was stored in the
    /// Qdrant point payload at write time (graph-builder `persist_graph_mutation`
    /// stores it as `payload["payload"]["skill_id"]`).
    pub skill_stable_id: String,
    /// RRF-fused score from Qdrant's dense+sparse prefetch fusion step.
    /// Used as the `lexical_score` in `FusedCandidate` for observability.
    pub fused_score: f32,
}

/// Error from a `HybridCandidateSource::query_hybrid` call.
///
/// A transport/status error means Qdrant is unreachable or returned an
/// unexpected response. The caller (orchestrator) must fail loud — not
/// silently fall back to the snapshot-dense path.
#[derive(Debug)]
pub enum HybridQueryError {
    /// Network/transport failure reaching Qdrant.
    Transport(String),
    /// Qdrant returned an unexpected HTTP status or error body.
    Status(String),
}

impl std::fmt::Display for HybridQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(msg) => write!(f, "qdrant hybrid query transport error: {msg}"),
            Self::Status(msg) => write!(f, "qdrant hybrid query returned error status: {msg}"),
        }
    }
}

/// Provides dense+sparse hybrid candidates from Qdrant at query time.
///
/// Implemented in `mcp-server` (wrapping `QdrantAdapter::query_hybrid` and
/// `model_keyed_hybrid_collection_name`). Injected into `RetrievalOrchestrator`
/// as `Option<Arc<dyn HybridCandidateSource>>` — `Some` only when
/// `RETRIEVAL_BACKEND=qdrant_hybrid`. Absent on the snapshot arms.
///
/// # Contract
///
/// Implementations MUST:
/// - Call the real Qdrant query API (no fakes, stubs, or mocks in production).
/// - Extract `skill_stable_id` from the point payload field `payload["payload"]["skill_id"]`.
/// - Return an `Err(HybridQueryError)` on any transport or status failure.
///   The orchestrator will surface this as a loud degraded outcome.
///
/// Implementations MUST NOT:
/// - Fall back silently to a stub or empty result on error.
/// - Cache results across calls (each retrieve call gets a fresh Qdrant query).
#[async_trait]
pub trait HybridCandidateSource: Send + Sync {
    /// Executes a dense+sparse hybrid query and returns ranked `HybridCandidate`s.
    ///
    /// `dense`: the prompt embedding vector (same model as the indexed dense vectors).
    /// `sparse_indices` / `sparse_values`: BM25-style sparse query vector.
    /// `limit`: the maximum number of results to return (≥ candidate_limit).
    ///
    /// Returns `HybridCandidate`s ordered by descending RRF fused score, limited
    /// to `limit` results. An empty `Ok(vec![])` is valid when no skills match.
    async fn query_hybrid(
        &self,
        dense: &[f32],
        sparse_indices: &[u32],
        sparse_values: &[f32],
        limit: u64,
    ) -> Result<Vec<HybridCandidate>, HybridQueryError>;
}
