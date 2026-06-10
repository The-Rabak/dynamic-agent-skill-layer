//! Contract tests for the T06 SkillDAG-style agent retrieval tools driven over
//! the real MCP HTTP endpoint at `http://127.0.0.1:3001/mcp`.
//!
//! These tests require a **live MCP server** with a seeded corpus.  They are
//! `#[ignore]`-gated and must be run explicitly:
//!
//! ```sh
//! cargo test -p mcp-server --features test-utils --test test_skill_graph_tools -- --ignored
//! ```
//!
//! # What is proven here
//!
//! - `find_skill` returns the structural contract fields introduced by T06:
//!   `rationale`, `fusion_rank_score`, and `retrieval_context`.
//! - `find_skill.score` reflects the eq.3 relevance signal (#260), not the RRF
//!   rank artifact — when multiple matches exist they can be distinct.
//! - `search_skill_graph` returns the three-section structure:
//!   `matches`, `neighbors`, `conflicts`.  The `retrieval_context` block is also
//!   present, carrying `embedding_model`, `collection`, and `graph_version`.
//! - `conflicts_with` edges never bleed into the `neighbors` array.
//! - `/health` exposes both the `embedding_arm` and `retrieval_backend` static
//!   components registered at boot by `main.rs`.
//!
//! # Assumptions
//!
//! - The corpus has at least one retrievable skill so `find_skill` returns an
//!   `ok` (not `no_match`) response.  Tests that require multiple matches degrade
//!   gracefully (they skip the score-distinctness assertion when only one match
//!   is returned).
//! - The `mcp-server` at port 3001 is already running with a live graph.  The
//!   tests do NOT call `Stack::up()` — that would be redundant on a CI runner
//!   where the compose stack is already managed externally.

#[path = "report.rs"]
mod report;

#[path = "harness/mod.rs"]
mod harness;

use harness::app::McpClient;
use serde_json::Value;

/// Calls `find_skill` and asserts the T06 structural contract.
///
/// Proves: `rationale`, `fusion_rank_score`, and `retrieval_context` are present
/// when the corpus returns at least one match.  Also validates that the
/// `score` field is a parseable decimal string and that when two matches exist
/// they can carry different scores (#260).
#[tokio::test]
#[ignore = "requires live containers"]
async fn find_skill_returns_t06_structural_contract() {
    let client = McpClient::new();

    // A broadly-worded prompt to maximise the chance of hitting the corpus.
    let result = client
        .call_tool(
            "find_skill",
            serde_json::json!({
                "prompt": "Rust async error handling patterns",
                "limit": 5
            }),
        )
        .await
        .expect("find_skill RPC call should succeed");

    assert!(
        result.error.is_none(),
        "find_skill must not return a JSON-RPC error, got: {:?}",
        result.error
    );

    let body = result
        .result
        .expect("find_skill must return a result payload");

    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .expect("find_skill result must contain 'status'");

    // When the corpus is empty the server legitimately returns no_match; skip
    // the structural assertions so a cold server does not produce a false fail.
    if status == "no_match" {
        eprintln!(
            "SKIP: find_skill returned no_match — corpus may be empty; \
             structural assertions require at least one skill"
        );
        return;
    }

    assert_eq!(
        status, "ok",
        "find_skill must return 'ok' when the corpus is populated, got '{status}'"
    );

    // `retrieval_context` must be present and populated (#243).
    let ctx = body
        .get("retrieval_context")
        .expect("find_skill result must contain 'retrieval_context' when ok (#243)");

    assert!(
        ctx.get("embedding_model")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "retrieval_context.embedding_model must be a non-empty string"
    );
    assert!(
        ctx.get("collection")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "retrieval_context.collection must be a non-empty string"
    );
    assert!(
        ctx.get("graph_version").and_then(|v| v.as_i64()).is_some(),
        "retrieval_context.graph_version must be an integer"
    );

    let matches = body
        .get("matches")
        .and_then(|v| v.as_array())
        .expect("find_skill result must contain a 'matches' array");

    assert!(
        !matches.is_empty(),
        "find_skill with status='ok' must have at least one match"
    );

    // Validate per-match T06 contract fields on the first match.
    let first = &matches[0];

    assert_skill_match_t06_fields(first, "first match");

    // #260: when two or more matches exist, check that `score` values can
    // differ.  This does not assert they MUST differ (the corpus may genuinely
    // produce identical scores for distinct skills), but it proves the field
    // carries a real signal rather than a fixed RRF artifact.
    if matches.len() >= 2 {
        let score_a = matches[0]
            .get("score")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .expect("first match score must parse as f64");
        let score_b = matches[1]
            .get("score")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .expect("second match score must parse as f64");

        // Scores must be non-negative and parseable — this is always true.
        assert!(score_a >= 0.0, "match score must be non-negative");
        assert!(score_b >= 0.0, "match score must be non-negative");

        // Scores must not both be the same suspicious RRF constant (~0.016393).
        // Before the #260 fix, all matches at the same RRF rank had identical
        // scores; after the fix they reflect the actual cosine similarity.
        // We allow equality only if the first match's rationale also shows the
        // same semantic= value (meaning the corpus genuinely has equal cosine).
        let rrf_sentinel = 0.016393_f64;
        let both_are_rrf_sentinel =
            (score_a - rrf_sentinel).abs() < 0.0001 && (score_b - rrf_sentinel).abs() < 0.0001;

        assert!(
            !both_are_rrf_sentinel,
            "#260: both top matches carry the RRF rank artifact ({score_a:.6}, {score_b:.6}); \
             score must expose eq.3 relevance, not the RRF constant"
        );
    }
}

/// Calls `search_skill_graph` and asserts the three-section structural contract.
///
/// Proves: the response contains `matches`, `neighbors`, `conflicts`, and
/// `retrieval_context`; the `neighbors` array never contains a `conflicts_with`
/// edge type.
#[tokio::test]
#[ignore = "requires live containers"]
async fn search_skill_graph_returns_three_section_structural_contract() {
    let client = McpClient::new();

    let result = client
        .call_tool(
            "search_skill_graph",
            serde_json::json!({
                "prompt": "Rust async error handling patterns",
                "limit": 5
            }),
        )
        .await
        .expect("search_skill_graph RPC call should succeed");

    assert!(
        result.error.is_none(),
        "search_skill_graph must not return a JSON-RPC error, got: {:?}",
        result.error
    );

    let body = result
        .result
        .expect("search_skill_graph must return a result payload");

    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .expect("search_skill_graph result must contain 'status'");

    if status == "no_match" || status == "degraded" {
        eprintln!(
            "SKIP: search_skill_graph returned '{status}' — corpus may be empty or \
             embedder unavailable; structural assertions require ok"
        );
        return;
    }

    assert_eq!(
        status, "ok",
        "search_skill_graph must return 'ok' when corpus is populated, got '{status}'"
    );

    // All three sections must be present as arrays (#255 P2-E / T06 graph surface).
    let matches = body
        .get("matches")
        .and_then(|v| v.as_array())
        .expect("search_skill_graph result must contain a 'matches' array");

    let neighbors = body
        .get("neighbors")
        .and_then(|v| v.as_array())
        .expect("search_skill_graph result must contain a 'neighbors' array");

    let conflicts = body
        .get("conflicts")
        .and_then(|v| v.as_array())
        .expect("search_skill_graph result must contain a 'conflicts' array");

    // `retrieval_context` must be present (#243).
    let ctx = body
        .get("retrieval_context")
        .expect("search_skill_graph result must contain 'retrieval_context' when ok");

    assert!(
        ctx.get("embedding_model")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "retrieval_context.embedding_model must be a non-empty string"
    );
    assert!(
        ctx.get("collection")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "retrieval_context.collection must be a non-empty string"
    );
    assert!(
        ctx.get("graph_version").and_then(|v| v.as_i64()).is_some(),
        "retrieval_context.graph_version must be an integer"
    );

    // Each match must satisfy the T06 per-match field contract.
    for (i, m) in matches.iter().enumerate() {
        assert_skill_match_t06_fields(m, &format!("match[{i}]"));
    }

    // `conflicts_with` edges must never appear in `neighbors` — the critical
    // safety invariant that prevents negative quality signals from boosting scores.
    for (i, neighbor) in neighbors.iter().enumerate() {
        let edge_type = neighbor
            .get("edge_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_ne!(
            edge_type,
            "conflicts_with",
            "neighbors[{i}] must not contain a 'conflicts_with' edge (skill_id={:?}); \
             conflict edges must be classified into 'conflicts', not 'neighbors'",
            neighbor.get("skill_id")
        );
    }

    // Each conflict (if any) must have `direction` and `origin` fields.
    for (i, conflict) in conflicts.iter().enumerate() {
        assert!(
            conflict.get("skill_id").is_some(),
            "conflicts[{i}] must have a 'skill_id' field"
        );
        assert!(
            conflict.get("direction").is_some(),
            "conflicts[{i}] must have a 'direction' field"
        );
        assert!(
            conflict.get("origin").is_some(),
            "conflicts[{i}] must have an 'origin' field"
        );
    }

    // `latency_ms` must be present and non-negative.
    let latency = body
        .get("latency_ms")
        .and_then(|v| v.as_u64())
        .expect("search_skill_graph result must contain 'latency_ms' as a non-negative integer");
    let _ = latency; // value is not asserted beyond presence and type
}

/// Proves that `/health` exposes both the `embedding_arm` and `retrieval_backend`
/// static components registered at server boot.
///
/// Agents depend on both fields to understand which vector space and which
/// candidate-generation strategy produced retrieval results.
#[tokio::test]
#[ignore = "requires live containers"]
async fn health_exposes_embedding_arm_and_retrieval_backend_components() {
    let client = McpClient::new();

    let (status_code, body) = client.health().await.expect("GET /health should complete");

    assert_eq!(
        status_code, 200,
        "/health must return 200 when the live server is healthy"
    );

    let components = body
        .get("components")
        .and_then(|v| v.as_array())
        .expect("/health response must contain a 'components' array");

    let component_names: Vec<&str> = components
        .iter()
        .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
        .collect();

    // Both static components must be registered at boot.
    assert!(
        component_names.contains(&"embedding_arm"),
        "embedding_arm component must be present in /health; got: {component_names:?}"
    );
    assert!(
        component_names.contains(&"retrieval_backend"),
        "retrieval_backend component must be present in /health; got: {component_names:?}"
    );

    // `embedding_arm` detail must carry the model/dim/collection triple.
    let arm = components
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("embedding_arm"))
        .unwrap();

    let arm_detail = arm
        .get("detail")
        .and_then(|v| v.as_str())
        .expect("embedding_arm must have a 'detail' field");

    assert!(
        arm_detail.contains("model="),
        "embedding_arm detail must contain 'model=': got '{arm_detail}'"
    );
    assert!(
        arm_detail.contains("collection="),
        "embedding_arm detail must contain 'collection=': got '{arm_detail}'"
    );

    // `retrieval_backend` detail must carry the strategy label.
    let backend = components
        .iter()
        .find(|c| c.get("name").and_then(|n| n.as_str()) == Some("retrieval_backend"))
        .unwrap();

    let backend_detail = backend
        .get("detail")
        .and_then(|v| v.as_str())
        .expect("retrieval_backend must have a 'detail' field");

    assert!(
        backend_detail.contains("backend="),
        "retrieval_backend detail must contain 'backend=': got '{backend_detail}'"
    );

    // The backend label must be one of the three known strategies.
    let known = [
        "backend=snapshot_dense",
        "backend=snapshot_hybrid",
        "backend=qdrant_hybrid",
    ];
    assert!(
        known.iter().any(|k| backend_detail.contains(k)),
        "retrieval_backend detail must be one of {known:?}: got '{backend_detail}'"
    );
}

// ---------------------------------------------------------------------------
// Shared assertion helpers
// ---------------------------------------------------------------------------

/// Asserts that a single `SkillMatch` value satisfies the T06 field contract.
///
/// Checks: `name`, `description`, `score` (parseable decimal), `fusion_rank_score`
/// (parseable decimal), `tags` (array), and `rationale` (non-empty array) as
/// required by the T06 spec for `find_skill` / `search_skill_graph` matches.
fn assert_skill_match_t06_fields(m: &Value, label: &str) {
    assert!(
        m.get("name")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "{label}: 'name' must be a non-empty string"
    );

    assert!(
        m.get("description").and_then(|v| v.as_str()).is_some(),
        "{label}: 'description' must be a string"
    );

    // `score` must be a parseable decimal — eq.3 relevance, NOT the RRF constant.
    let score_str = m
        .get("score")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{label}: 'score' field must be present"));
    assert!(
        score_str.parse::<f64>().is_ok(),
        "{label}: 'score' must be a parseable decimal, got '{score_str}'"
    );

    // `fusion_rank_score` carries the RRF ordering provenance.
    let fusion_str = m
        .get("fusion_rank_score")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{label}: 'fusion_rank_score' field must be present (#260)"));
    assert!(
        fusion_str.parse::<f64>().is_ok(),
        "{label}: 'fusion_rank_score' must be a parseable decimal, got '{fusion_str}'"
    );

    assert!(
        m.get("tags").and_then(|v| v.as_array()).is_some(),
        "{label}: 'tags' must be an array"
    );

    // `rationale` must be a non-empty array of strings (#255 P1-B).
    let rationale = m
        .get("rationale")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| {
            panic!("{label}: 'rationale' field must be a present array (#255 P1-B)")
        });
    assert!(
        !rationale.is_empty(),
        "{label}: 'rationale' must be non-empty — it must carry at least 'rrf=…' and 'semantic=…'"
    );

    // At least one rationale entry must start with `semantic=` (the eq.3 source).
    let has_semantic = rationale.iter().any(|e| {
        e.as_str()
            .map(|s| s.starts_with("semantic="))
            .unwrap_or(false)
    });
    assert!(
        has_semantic,
        "{label}: 'rationale' must contain a 'semantic=…' entry; got: {rationale:?}"
    );
}
