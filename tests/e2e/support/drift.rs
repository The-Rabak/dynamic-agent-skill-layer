// Suppress dead-code warnings: these functions are public test-support APIs
// intended for use by sibling slice tests (DS-005 and peers), not just the
// smoke test. The compiler cannot see those future call sites yet.
// Review for a genuine orphan before deleting any helper only one non-gate binary uses.
#![allow(dead_code)]

/// PG/Qdrant drift injection helpers for the real-infra E2E harness.
///
/// These helpers manufacture two kinds of drift that the reconciler must repair:
///
/// - **PG-only skills** (`inject_pg_skills_without_qdrant_vectors`): rows in the
///   `skills` table with no matching Qdrant vector. Used by DS-005 to prove that
///   the reconciler detects PG→Qdrant divergence and closes the gap.
///
/// - **Qdrant-only vectors** (`inject_qdrant_vectors_without_pg_rows`): vectors
///   in the Qdrant collection with no corresponding PG skill row. Used by DS-005
///   to prove detection of the reverse direction.
///
/// Both functions return the injected IDs so callers can assert divergence counts
/// and then clean up with [`remove_injected_skills`] / [`remove_injected_qdrant_vectors`].
///
/// These helpers insert raw rows directly — they intentionally bypass the
/// outbox relay so divergence is immediately measurable.
use std::collections::HashSet;

use reqwest::Client as HttpClient;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

/// A record representing a PG skill row that was injected as drift.
#[derive(Debug, Clone)]
pub struct InjectedPgSkill {
    /// The UUID used as the skill's primary key.
    pub skill_id: Uuid,
    /// Stable-ID string stored in the `name` column for identification.
    pub marker_name: String,
}

/// Inserts `count` skill rows directly into Postgres without creating any
/// corresponding Qdrant vectors, producing PG→Qdrant drift.
///
/// Each injected skill is marked with a name prefix (`drift-inject-`) so callers
/// can identify and clean them up. The rows are inserted as `lifecycle = 'active'`
/// and `status = 'ready'` to match what the normal write path produces.
///
/// Returns the list of injected records so callers can assert divergence counts
/// and clean up with [`remove_injected_skills`].
pub async fn inject_pg_skills_without_qdrant_vectors(
    pool: &PgPool,
    count: usize,
) -> Result<Vec<InjectedPgSkill>, sqlx::Error> {
    let mut injected = Vec::with_capacity(count);

    for i in 0..count {
        let skill_id = Uuid::now_v7();
        let marker_name = format!("drift-inject-pg-only-{i}-{}", &skill_id.to_string()[..8]);

        sqlx::query(
            r#"
            INSERT INTO skills (id, name, description, scope, merged_from_scopes, status,
                                lifecycle, tags, source_paths, graph_version)
            VALUES ($1, $2, $3, 'global', '{}', 'ready', 'active', '{}', '{}', 0)
            "#,
        )
        .bind(skill_id)
        .bind(&marker_name)
        .bind("Drift-injection placeholder — no Qdrant vector counterpart")
        .execute(pool)
        .await?;

        injected.push(InjectedPgSkill {
            skill_id,
            marker_name,
        });
    }

    Ok(injected)
}

/// Removes all skill rows that were injected by [`inject_pg_skills_without_qdrant_vectors`].
///
/// Deletes by primary key so only the exact injected rows are removed. Returns the
/// count of rows deleted.
pub async fn remove_injected_skills(
    pool: &PgPool,
    injected: &[InjectedPgSkill],
) -> Result<u64, sqlx::Error> {
    if injected.is_empty() {
        return Ok(0);
    }

    let ids: Vec<Uuid> = injected.iter().map(|s| s.skill_id).collect();
    let result = sqlx::query("DELETE FROM skills WHERE id = ANY($1)")
        .bind(&ids)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}

/// An opaque point ID for a Qdrant-only vector injected as drift.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InjectedQdrantPointId(pub u64);

/// Upserts `count` vectors directly into Qdrant with no corresponding PG skill row,
/// producing Qdrant→PG drift.
///
/// Point IDs are derived from a deterministic hash of a marker string to avoid
/// collisions with real production vectors. The marker string is embedded in the
/// payload so callers can identify and remove only the injected points.
///
/// Returns the list of injected point IDs so callers can clean up with
/// [`remove_injected_qdrant_vectors`].
pub async fn inject_qdrant_vectors_without_pg_rows(
    qdrant_base_url: &str,
    collection_name: &str,
    vector_size: usize,
    count: usize,
) -> Result<Vec<InjectedQdrantPointId>, String> {
    let client = HttpClient::new();
    let url = format!(
        "{}/collections/{collection_name}/points?wait=true",
        qdrant_base_url.trim_end_matches('/')
    );

    let mut injected = Vec::with_capacity(count);

    for i in 0..count {
        // Use a hash-derived point_id in a range unlikely to overlap production IDs.
        // We use a large offset (u64::MAX / 4) to push into unpopulated space.
        let marker = format!("drift-inject-qdrant-only-{i}");
        let point_id = drift_point_id(&marker);

        let dummy_vector: Vec<f32> = (0..vector_size)
            .map(|j| (i as f32 * 0.01 + j as f32 * 0.001).min(1.0))
            .collect();

        let body = json!({
            "points": [{
                "id": point_id,
                "vector": dummy_vector,
                "payload": {
                    "drift_marker": marker,
                    "pg_only": false,
                    "qdrant_only": true
                }
            }]
        });

        let response = client
            .put(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("qdrant upsert request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "qdrant upsert returned unexpected status {}",
                response.status()
            ));
        }

        injected.push(InjectedQdrantPointId(point_id));
    }

    Ok(injected)
}

/// Deletes vectors injected by [`inject_qdrant_vectors_without_pg_rows`] from Qdrant.
///
/// Sends a DELETE request to the Qdrant scroll/points endpoint for the exact
/// point IDs that were injected. Returns the number of points confirmed deleted.
pub async fn remove_injected_qdrant_vectors(
    qdrant_base_url: &str,
    collection_name: &str,
    injected: &[InjectedQdrantPointId],
) -> Result<usize, String> {
    if injected.is_empty() {
        return Ok(0);
    }

    let client = HttpClient::new();
    let url = format!(
        "{}/collections/{collection_name}/points/delete?wait=true",
        qdrant_base_url.trim_end_matches('/')
    );

    let ids: Vec<u64> = injected.iter().map(|p| p.0).collect();
    let body = json!({ "points": ids });

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("qdrant delete request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "qdrant delete returned unexpected status {}",
            response.status()
        ));
    }

    Ok(injected.len())
}

/// Computes the set of skill UUIDs present in Postgres (active lifecycle only).
///
/// Used by callers to measure the PG→Qdrant gap after drift injection.
pub async fn pg_active_skill_ids(pool: &PgPool) -> Result<HashSet<String>, sqlx::Error> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT id::TEXT FROM skills WHERE lifecycle = 'active'")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Derives a deterministic `u64` point ID from a marker string.
///
/// Uses a simple FNV-1a hash to produce a stable ID that is unlikely to
/// collide with real production point IDs (which the normal write path derives
/// from content hashes in a different range).
fn drift_point_id(marker: &str) -> u64 {
    // FNV-1a 64-bit hash. Offset chosen to land near u64::MAX / 2, away from
    // real production IDs that start at small values from content-hash truncation.
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
    const HIGH_RANGE_BASE: u64 = u64::MAX / 2;

    let hash = marker.bytes().fold(FNV_OFFSET, |acc, byte| {
        (acc ^ byte as u64).wrapping_mul(FNV_PRIME)
    });

    // Fold into high range, preserving bit diversity.
    HIGH_RANGE_BASE.wrapping_add(hash >> 1)
}
