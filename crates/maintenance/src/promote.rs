use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
    sync::Arc,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::{
    EmbeddingService, PENDING_SKILL_FILE_NAME, ScopeType, pending_default_expires_at,
    pending_default_warning_at,
};
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::cron::{CronError, DemotionPassRunner, PromotionPassRunner};
use crate::merge::{
    MergeConfig, MergeError, SkillSnapshot, canonicalize_scope_root,
    ensure_path_is_within_scope_root, validate_pending_directory_name,
};
use infrastructure::{
    GlobalSkillRow, LlmEquivalenceVerifier, ProjectSkillRow, PromotionRecurrenceStore,
    ScopeDemotionStore,
};

// ─── Evidence ─────────────────────────────────────────────────────────────────

/// The signal that drove the promotion decision.
///
/// Only `Intrinsic` is constructed in this slice (todo #179). `Recurrence` is
/// defined here so todo #180 can populate it without a breaking struct change.
#[derive(Debug, Clone, PartialEq)]
pub enum PromotionEvidence {
    /// The skill text contains no project-local identifiers AND the LLM confirmed
    /// the lesson is tool/language-general.
    Intrinsic,
    /// The same lesson recurred across `project_count` distinct project roots.
    /// Populated by the recurrence path (todo #180).
    Recurrence { project_count: usize },
}

// ─── Proposal ─────────────────────────────────────────────────────────────────

/// Propose-only promotion artifact: a `.pending` file inside the global skill root.
///
/// Nothing is applied automatically. A human renames the file to approve it.
#[derive(Debug, Clone, PartialEq)]
pub struct PromotionProposal {
    /// Absolute path of the written `.pending` file (inside the global scope root).
    pub pending_path: PathBuf,
    /// IDs of the source skills contributing to this proposal.
    pub skill_ids: Vec<String>,
    /// Scope(s) the contributing skills were promoted FROM.
    pub from_scopes: Vec<ScopeType>,
    /// Destination scope — always `Global` for promotion.
    pub to_scope: ScopeType,
    /// Evidence that drove the promotion decision.
    pub evidence: PromotionEvidence,
    /// Filesystem paths of the source skill files (for audit and frontmatter provenance).
    pub source_paths: Vec<PathBuf>,
}

// ─── Scope policy ─────────────────────────────────────────────────────────────

/// Scope policy for promotion proposals.
///
/// Promotion is always upward to `Global`. This is NOT a reuse of
/// `ScopeSelectionPolicy::PreferProjectThenGlobal` from `merge.rs`, which
/// canonicalises downward (toward Project). Promotion is the inverse direction
/// and must resolve independently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromotionScopePolicy;

impl PromotionScopePolicy {
    /// Resolves the target scope for a promotion proposal — always `Global`.
    pub fn to_scope(self) -> ScopeType {
        ScopeType::Global
    }
}

// ─── Identifier check ─────────────────────────────────────────────────────────

/// Returns `true` if `skill_text` references a project-local identifier.
///
/// A "project-local identifier" is defined as any token from `project_identifier_tokens`
/// — typically derived from the project root path components and known workspace
/// crate/symbol names — that appears verbatim in the skill text.
///
/// This is a deterministic veto: even a skill hinted `general` must be rejected for
/// global promotion if its text contains a project-local identifier.
///
/// Pure function; no I/O; safe to call in tests without infrastructure.
pub fn skill_text_contains_project_local_identifier(
    skill_text: &str,
    project_identifier_tokens: &[&str],
) -> bool {
    for token in project_identifier_tokens {
        if token.is_empty() {
            continue;
        }
        if skill_text.contains(*token) {
            debug!(
                token,
                "promotion identifier check: project-local token found — vetoing promotion"
            );
            return true;
        }
    }
    false
}

/// Collects the project-local identifiers found in `skill_text`.
///
/// Functionally the same predicate as [`skill_text_contains_project_local_identifier`]
/// but returns the matched tokens rather than a boolean — used by the demotion
/// pass to record WHICH identifiers triggered the flag (the todo invariant requires
/// cited evidence, not just a true/false answer).
///
/// Returns a deduplicated, sorted list of matching tokens for deterministic output.
/// An empty return value means no project-local identifiers were found.
///
/// Pure function; no I/O; safe to call in tests without infrastructure.
pub fn collect_project_local_identifiers(
    skill_text: &str,
    project_identifier_tokens: &[&str],
) -> Vec<String> {
    let mut found: Vec<String> = project_identifier_tokens
        .iter()
        .filter(|token| !token.is_empty() && skill_text.contains(**token))
        .map(|token| (*token).to_owned())
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

// ─── Demotion proposal ────────────────────────────────────────────────────────

/// Propose-only demotion artifact: a `.pending` file inside the global skill root
/// flagging a global skill as mis-scoped.
///
/// A global skill that references project-local identifiers pollutes every project's
/// retrieval at weight 0.7. This proposal flags it for human review — nothing is
/// applied automatically. A human renames the file to approve demotion to project scope.
#[derive(Debug, Clone, PartialEq)]
pub struct DemotionProposal {
    /// Absolute path of the written `.pending` file (inside the global scope root).
    pub pending_path: PathBuf,
    /// ID of the global skill flagged for demotion.
    pub skill_id: String,
    /// Source scope — always `Global` for demotion (the skill is currently global).
    pub from_scope: ScopeType,
    /// Destination scope — always `Project` for demotion.
    pub to_scope: ScopeType,
    /// The project-local identifier tokens found in the skill text.
    ///
    /// Non-empty by construction: a demotion proposal is only emitted when at
    /// least one offending identifier is found. These tokens ARE the evidence —
    /// no opaque "feels project-specific" without citation.
    pub offending_identifiers: Vec<String>,
}

// ─── Proposal writer ──────────────────────────────────────────────────────────

/// Writes global `.pending` promotion artifacts without mutating source skills.
///
/// Path-safety is enforced via the same `ensure_path_is_within_scope_root` and
/// `create_new(true)` no-overwrite write that `MergeProposalWriter` uses. These
/// helpers are shared (reused from `merge.rs` as `pub(crate)`) — not forked.
pub struct PromotionProposalWriter {
    /// Absolute path of the global scope root where proposals are written.
    global_scope_root: PathBuf,
    /// Directory name for the `.pending` subdirectory (e.g. `.skills`).
    pending_directory_name: String,
}

impl PromotionProposalWriter {
    /// Constructs a writer anchored to the given global scope root.
    ///
    /// The `global_scope_root` must be an absolute path to an existing directory —
    /// `write_proposal` will fail with `PromotionError::InvalidGlobalScopeRoot`
    /// if this is not satisfied.
    pub fn new(global_scope_root: PathBuf, pending_directory_name: String) -> Self {
        Self {
            global_scope_root,
            pending_directory_name,
        }
    }

    /// Writes a global `.pending` artifact for a promotion candidate.
    ///
    /// The proposal is written inside `<global_scope_root>/<pending_directory_name>/`
    /// in a subdirectory named after the source skill. Path confinement is enforced
    /// before the write — the function returns `PromotionError::WritePathOutsideScopeRoot`
    /// if the resolved path escapes the global scope root.
    ///
    /// Uses `create_new(true)` so a pre-existing proposal for the same skill is
    /// never silently overwritten. Callers that want idempotency must remove stale
    /// proposals before calling this.
    pub fn write_proposal(
        &self,
        snapshot: &SkillSnapshot,
        evidence: PromotionEvidence,
        now: DateTime<Utc>,
    ) -> Result<PromotionProposal, PromotionError> {
        // Canonicalize the global scope root — requires the directory to exist.
        let canonical_root = canonicalize_scope_root(&self.global_scope_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_dir_name = validate_pending_directory_name(&self.pending_directory_name)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_root = canonical_root.join(pending_dir_name);
        fs::create_dir_all(&pending_root).map_err(|error| PromotionError::WriteFailure {
            path: pending_root.display().to_string(),
            message: error.to_string(),
        })?;

        let canonical_pending_root =
            pending_root
                .canonicalize()
                .map_err(|error| PromotionError::WriteFailure {
                    path: pending_root.display().to_string(),
                    message: error.to_string(),
                })?;

        // Enforce path confinement — reusing the same logic as merge.rs.
        ensure_path_is_within_scope_root(&canonical_pending_root, &canonical_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let proposal_dir = canonical_pending_root.join(proposal_directory_name(&snapshot.id));
        fs::create_dir_all(&proposal_dir).map_err(|error| PromotionError::WriteFailure {
            path: proposal_dir.display().to_string(),
            message: error.to_string(),
        })?;

        // Verify the proposal subdirectory is also within the scope root.
        let canonical_proposal_dir =
            proposal_dir
                .canonicalize()
                .map_err(|error| PromotionError::WriteFailure {
                    path: proposal_dir.display().to_string(),
                    message: error.to_string(),
                })?;
        ensure_path_is_within_scope_root(&canonical_proposal_dir, &canonical_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_path = canonical_proposal_dir.join(PENDING_SKILL_FILE_NAME);
        let pending_body = render_pending_markdown(snapshot, &evidence, now)?;

        // create_new(true): never silently overwrite a pre-existing proposal.
        let mut pending_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending_path)
            .map_err(|error| PromotionError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;
        pending_file
            .write_all(pending_body.as_bytes())
            .map_err(|error| PromotionError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;

        Ok(PromotionProposal {
            pending_path,
            skill_ids: vec![snapshot.id.clone()],
            from_scopes: vec![snapshot.scope],
            to_scope: PromotionScopePolicy.to_scope(),
            evidence,
            source_paths: vec![snapshot.source_path.clone()],
        })
    }

    /// Writes a global `.pending` artifact for a demotion candidate.
    ///
    /// The proposal is written inside `<global_scope_root>/<pending_directory_name>/`
    /// in a subdirectory named `demote--{skill-id-slug}`. Path confinement is enforced
    /// before the write — returns `PromotionError::WritePathOutsideScopeRoot` if the
    /// resolved path escapes the global scope root.
    ///
    /// Uses `create_new(true)` so a pre-existing proposal for the same skill is never
    /// silently overwritten.
    ///
    /// The `offending_identifiers` must be non-empty — callers must only call this
    /// when at least one identifier was found.
    pub fn write_demotion_proposal(
        &self,
        skill: &GlobalSkillRow,
        offending_identifiers: Vec<String>,
        now: DateTime<Utc>,
    ) -> Result<DemotionProposal, PromotionError> {
        // Canonicalize the global scope root — requires the directory to exist.
        let canonical_root = canonicalize_scope_root(&self.global_scope_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_dir_name = validate_pending_directory_name(&self.pending_directory_name)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_root = canonical_root.join(pending_dir_name);
        fs::create_dir_all(&pending_root).map_err(|error| PromotionError::WriteFailure {
            path: pending_root.display().to_string(),
            message: error.to_string(),
        })?;

        let canonical_pending_root =
            pending_root
                .canonicalize()
                .map_err(|error| PromotionError::WriteFailure {
                    path: pending_root.display().to_string(),
                    message: error.to_string(),
                })?;

        // Enforce path confinement — reusing the same logic as merge.rs.
        ensure_path_is_within_scope_root(&canonical_pending_root, &canonical_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let proposal_dir = canonical_pending_root.join(demotion_directory_name(&skill.id));
        fs::create_dir_all(&proposal_dir).map_err(|error| PromotionError::WriteFailure {
            path: proposal_dir.display().to_string(),
            message: error.to_string(),
        })?;

        // Verify the proposal subdirectory is also within the scope root.
        let canonical_proposal_dir =
            proposal_dir
                .canonicalize()
                .map_err(|error| PromotionError::WriteFailure {
                    path: proposal_dir.display().to_string(),
                    message: error.to_string(),
                })?;
        ensure_path_is_within_scope_root(&canonical_proposal_dir, &canonical_root)
            .map_err(PromotionError::ScopeRootInvalid)?;

        let pending_path = canonical_proposal_dir.join(PENDING_SKILL_FILE_NAME);
        let pending_body = render_demotion_pending_markdown(skill, &offending_identifiers, now)?;

        // create_new(true): never silently overwrite a pre-existing proposal.
        let mut pending_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending_path)
            .map_err(|error| PromotionError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;
        pending_file
            .write_all(pending_body.as_bytes())
            .map_err(|error| PromotionError::WriteFailure {
                path: pending_path.display().to_string(),
                message: error.to_string(),
            })?;

        Ok(DemotionProposal {
            pending_path,
            skill_id: skill.id.clone(),
            from_scope: ScopeType::Global,
            to_scope: ScopeType::Project,
            offending_identifiers,
        })
    }
}

// ─── Live runner ──────────────────────────────────────────────────────────────

/// Live promotion pass runner: runs BOTH the intrinsic path (todo #179) and the
/// cross-project recurrence path (todo #180), merging their proposals.
///
/// Also implements [`DemotionPassRunner`] (todo #182): scans `scope='global'` skills
/// from PG for project-local identifier references and emits demotion proposals for
/// any that are found to be mis-scoped.
///
/// **Intrinsic path**: queries approved project skills from the filesystem snapshot,
/// runs the identifier veto + LLM generality vote, emits proposals for skills that
/// reference no project-local identifiers and are confirmed general.
///
/// **Recurrence path**: reads `scope='project'` skills across ALL roots from PG,
/// re-embeds each skill's semantic text, clusters near-duplicates by cosine
/// similarity + LLM equivalence, and emits proposals for clusters that span ≥N
/// distinct project-root keys. See [`RecurrenceConfig`] for threshold details.
///
/// **Demotion path** (todo #182): reads `scope='global'` skills from PG, runs the
/// same deterministic identifier check (inverse direction — a global skill SHOULD
/// NOT reference project-local identifiers), and emits demotion proposals for any
/// that are found to be mis-scoped.
pub struct LivePromotionPassRunner {
    /// Loaded skill snapshots to evaluate each pass (intrinsic path).
    pub skill_snapshots: Vec<SkillSnapshot>,
    /// LLM generality verifier (fails loud on provider error) — intrinsic path.
    pub generality_verifier: Arc<dyn infrastructure::SkillGeneralityVerifier>,
    /// Project-local identifier tokens used by the deterministic veto — intrinsic path.
    /// Typically derived from the project root path components and workspace symbol names.
    pub project_identifier_tokens: Vec<String>,
    /// Configuration for the proposal writer (scope root + directory name).
    pub promotion_writer_config: PromotionWriterConfig,
    // ─── Recurrence path (todo #180) ──────────────────────────────────────────
    /// PG store for fetching project skills across all roots — recurrence path.
    ///
    /// `None` disables the recurrence pass (single-project installs, or when the
    /// PG connection is not configured for the recurrence store). A `None` value
    /// is logged as a skipped recurrence pass, never as a silent success.
    pub recurrence_store: Option<Arc<dyn PromotionRecurrenceStore>>,
    /// Embedding service for re-embedding project-skill semantic text — recurrence path.
    ///
    /// Embeddings live in Qdrant (not the `skills` table), so the recurrence pass
    /// re-embeds via the same service the merge snapshot path uses. See module doc
    /// in `infrastructure::persistence::promotion_recurrence` for the design choice.
    pub embedding_service: Option<Arc<dyn EmbeddingService>>,
    /// LLM equivalence verifier for clustering near-duplicate skills — recurrence path.
    pub equivalence_verifier: Option<Arc<dyn LlmEquivalenceVerifier>>,
    /// Configuration controlling the recurrence threshold and similarity cutoff.
    pub recurrence_config: RecurrenceConfig,
    // ─── Demotion path (todo #182) ────────────────────────────────────────────
    /// PG store for fetching global skills to scan for mis-scoping — demotion path.
    ///
    /// `None` disables the demotion pass. When `None`, the pass is skipped with a
    /// clear info-level log — never a silent no-op.
    pub demotion_store: Option<Arc<dyn ScopeDemotionStore>>,
}

/// Configuration for the `PromotionProposalWriter`.
#[derive(Debug, Clone)]
pub struct PromotionWriterConfig {
    /// Absolute path of the global scope root.
    pub global_scope_root: PathBuf,
    /// Directory name for the `.pending` subdirectory (e.g. `.skills`).
    pub pending_directory_name: String,
}

impl Default for PromotionWriterConfig {
    fn default() -> Self {
        Self {
            global_scope_root: PathBuf::new(),
            pending_directory_name: MergeConfig::default().pending_directory_name,
        }
    }
}

// ─── Recurrence config ────────────────────────────────────────────────────────

/// Configuration for the cross-project recurrence detection path (todo #180).
///
/// Threshold N is read from the `PROMOTION_RECURRENCE_THRESHOLD` env variable at
/// runtime; this struct is the config carrier for unit-test injection.
#[derive(Debug, Clone, PartialEq)]
pub struct RecurrenceConfig {
    /// Minimum number of DISTINCT project-root keys a cluster must span before a
    /// `Recurrence` promotion proposal is emitted. Default 2; minimum 2 (a value
    /// of 1 would mean any single-project skill is promoted, which defeats the
    /// purpose of cross-project recurrence detection).
    pub min_distinct_roots: usize,
    /// Cosine-similarity threshold for grouping two skills into the same cluster.
    /// Mirrors the merge-pass default (0.85). Skills below this threshold are not
    /// compared with the LLM verifier.
    pub similarity_threshold: f32,
}

impl Default for RecurrenceConfig {
    fn default() -> Self {
        Self {
            min_distinct_roots: 2,
            similarity_threshold: 0.85,
        }
    }
}

impl RecurrenceConfig {
    /// Reads `min_distinct_roots` from `PROMOTION_RECURRENCE_THRESHOLD` env var.
    ///
    /// Falls back to the struct default (N=2) when the variable is unset or
    /// unparseable — and logs a warning for unparseable values so misconfiguration
    /// is visible.
    pub fn from_env() -> Self {
        let min_distinct_roots = match std::env::var("PROMOTION_RECURRENCE_THRESHOLD") {
            Ok(raw) => match raw.trim().parse::<usize>() {
                Ok(n) if n >= 2 => n,
                Ok(n) => {
                    warn!(
                        threshold = n,
                        "PROMOTION_RECURRENCE_THRESHOLD must be ≥2; using default 2"
                    );
                    2
                }
                Err(_) => {
                    warn!(
                        raw = %raw,
                        "PROMOTION_RECURRENCE_THRESHOLD is not a valid integer; using default 2"
                    );
                    2
                }
            },
            Err(_) => 2,
        };
        Self {
            min_distinct_roots,
            ..Self::default()
        }
    }
}

#[async_trait]
impl PromotionPassRunner for LivePromotionPassRunner {
    async fn run_promotion_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PromotionProposal>, CronError> {
        // Run both paths and merge proposals.
        let intrinsic_proposals = self.run_intrinsic_pass(now).await?;
        let recurrence_proposals = self.run_recurrence_pass(now).await?;

        let mut proposals = intrinsic_proposals;
        proposals.extend(recurrence_proposals);
        Ok(proposals)
    }
}

#[async_trait]
impl DemotionPassRunner for LivePromotionPassRunner {
    async fn run_demotion_pass(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<Vec<DemotionProposal>, CronError> {
        self.scan_demotions(now).await
    }
}

impl LivePromotionPassRunner {
    /// Runs the intrinsic promotion path (todo #179).
    ///
    /// Evaluates filesystem-loaded skill snapshots: deterministic identifier veto
    /// + LLM generality vote. Emits `PromotionEvidence::Intrinsic` proposals for
    ///   skills that reference no project-local identifiers and are confirmed general.
    async fn run_intrinsic_pass(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PromotionProposal>, CronError> {
        // Only approved project-scoped skills are candidates.
        let project_candidates: Vec<&SkillSnapshot> = self
            .skill_snapshots
            .iter()
            .filter(|s| s.scope == ScopeType::Project)
            .collect();

        if project_candidates.is_empty() {
            debug!("promotion pass (intrinsic): no project-scoped skills to evaluate");
            return Ok(Vec::new());
        }

        let token_refs: Vec<&str> = self
            .project_identifier_tokens
            .iter()
            .map(String::as_str)
            .collect();

        let writer = PromotionProposalWriter::new(
            self.promotion_writer_config.global_scope_root.clone(),
            self.promotion_writer_config.pending_directory_name.clone(),
        );

        let mut proposals = Vec::new();

        for snapshot in project_candidates {
            let skill_text = snapshot.semantic_text();

            // Step 1: deterministic veto — skip if any project-local token is present.
            if skill_text_contains_project_local_identifier(&skill_text, &token_refs) {
                debug!(
                    skill_id = %snapshot.id,
                    "promotion pass (intrinsic): project-local identifier found — skipping"
                );
                continue;
            }

            // Step 2: LLM generality vote — fail loud on provider error.
            let decision = self
                .generality_verifier
                .decide_generality(&skill_text)
                .await
                .map_err(|error| {
                    CronError::PromotionPass(format!(
                        "generality verifier failed for skill `{}`: {error}",
                        snapshot.id
                    ))
                })?;

            if !decision.general {
                debug!(
                    skill_id = %snapshot.id,
                    rationale = %decision.rationale,
                    "promotion pass (intrinsic): LLM decided not general — skipping"
                );
                continue;
            }

            debug!(
                skill_id = %snapshot.id,
                rationale = %decision.rationale,
                "promotion pass (intrinsic): intrinsic gate passed — writing proposal"
            );

            let proposal = writer
                .write_proposal(snapshot, PromotionEvidence::Intrinsic, now)
                .map_err(|error| CronError::PromotionPass(error.to_string()))?;

            proposals.push(proposal);
        }

        Ok(proposals)
    }

    /// Runs the cross-project recurrence promotion path (todo #180).
    ///
    /// Reads all `scope='project'` skills from PG (across ALL roots), re-embeds
    /// each skill's semantic text, clusters near-duplicates by cosine similarity +
    /// LLM equivalence, and emits `PromotionEvidence::Recurrence` proposals for
    /// clusters spanning ≥N distinct project-root keys.
    ///
    /// **Mandatory log**: EVERY invocation logs `distinct_project_roots_seen` vs
    /// the threshold (invariant from design caveat #1). A single-project install
    /// will see "1 root seen, threshold 2 — nothing to promote yet" rather than
    /// a silent no-op.
    ///
    /// # Errors
    ///
    /// Returns `CronError::PromotionPass` if the PG store query fails, the
    /// embedding service fails, or the equivalence verifier fails. Never swallows
    /// these errors — they surface with a `reason_code` via `CronError`.
    async fn run_recurrence_pass(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<PromotionProposal>, CronError> {
        // Recurrence pass requires all three components. Log clearly when any is absent.
        let (recurrence_store, embedding_service, equivalence_verifier) = match (
            self.recurrence_store.as_ref(),
            self.embedding_service.as_ref(),
            self.equivalence_verifier.as_ref(),
        ) {
            (Some(store), Some(emb), Some(verifier)) => (store, emb, verifier),
            _ => {
                info!(
                    distinct_project_roots_seen = 0_usize,
                    threshold = self.recurrence_config.min_distinct_roots,
                    "promotion pass (recurrence): store/embedding/verifier not configured \
                     — recurrence pass skipped; configure PROMOTION_RECURRENCE_STORE \
                     to enable cross-project recurrence detection"
                );
                return Ok(Vec::new());
            }
        };

        // Fetch all project skills across ALL roots from PG.
        let project_skills =
            recurrence_store
                .fetch_all_project_skills()
                .await
                .map_err(|error| {
                    CronError::PromotionPass(format!(
                        "recurrence pass: PG store query failed \
                     [reason_code=promotion_recurrence_db_error]: {error}"
                    ))
                })?;

        // Count distinct project-root keys to satisfy the mandatory log invariant
        // (design caveat #1): every run must log roots_seen vs threshold.
        let distinct_project_roots: HashSet<String> = project_skills
            .iter()
            .filter_map(|row| row.project_root_key())
            .collect();

        let roots_seen = distinct_project_roots.len();
        let threshold = self.recurrence_config.min_distinct_roots;

        // MANDATORY log (design caveat #1): always visible, even in single-project installs.
        info!(
            distinct_project_roots_seen = roots_seen,
            threshold = threshold,
            total_project_skills = project_skills.len(),
            "promotion pass (recurrence): cross-project aggregate scan complete"
        );

        if roots_seen < threshold {
            info!(
                distinct_project_roots_seen = roots_seen,
                threshold = threshold,
                "promotion pass (recurrence): only {roots_seen} distinct project root(s) seen \
                 — threshold {threshold} not met; nothing to promote yet"
            );
            return Ok(Vec::new());
        }

        // Re-embed each skill's semantic text for cosine clustering.
        // Embeddings are in Qdrant (not the `skills` table); re-embed here via the
        // same EmbeddingService the merge snapshot path uses. This avoids coupling
        // to Qdrant and is the accepted stub-free default per design triage.
        let mut embedded_skills: Vec<(ProjectSkillRow, Vec<f32>)> =
            Vec::with_capacity(project_skills.len());

        for row in project_skills {
            let text = row.semantic_text();
            let embedding = embedding_service.embed_text(&text).await.map_err(|error| {
                CronError::PromotionPass(format!(
                    "recurrence pass: embedding failed for skill `{}` \
                         [reason_code=promotion_recurrence_embedding_error]: {error}",
                    row.id
                ))
            })?;
            embedded_skills.push((row, embedding));
        }

        // Cluster near-duplicate skills by cosine similarity + LLM equivalence.
        // A cluster is a group of skills whose pairwise similarity meets the
        // threshold AND the LLM confirms equivalence. We use a greedy union-find:
        // for each pair, if similarity ≥ threshold AND LLM says equivalent,
        // merge them into the same cluster.
        let clusters = build_recurrence_clusters(
            &embedded_skills,
            self.recurrence_config.similarity_threshold,
            equivalence_verifier.as_ref(),
        )
        .await
        .map_err(|error| {
            CronError::PromotionPass(format!(
                "recurrence pass: clustering failed \
                 [reason_code=promotion_recurrence_cluster_error]: {error}"
            ))
        })?;

        // Emit proposals for clusters spanning ≥N distinct project-root keys.
        let writer = PromotionProposalWriter::new(
            self.promotion_writer_config.global_scope_root.clone(),
            self.promotion_writer_config.pending_directory_name.clone(),
        );

        let mut proposals = Vec::new();

        for cluster in clusters {
            let distinct_roots: HashSet<String> = cluster
                .iter()
                .filter_map(|row| row.project_root_key())
                .collect();

            let cluster_root_count = distinct_roots.len();

            if cluster_root_count < threshold {
                debug!(
                    cluster_size = cluster.len(),
                    distinct_roots = cluster_root_count,
                    threshold,
                    "promotion pass (recurrence): cluster spans < {threshold} distinct roots — skipping"
                );
                continue;
            }

            // Use the first skill in the cluster as the representative for the proposal.
            // The recurrence evidence carries `project_count` so the human reviewer
            // knows how many projects contributed.
            let representative = &cluster[0];
            let representative_snapshot = project_skill_row_to_snapshot(representative);

            info!(
                skill_id = %representative.id,
                cluster_size = cluster.len(),
                distinct_roots = cluster_root_count,
                "promotion pass (recurrence): cluster qualifies — writing proposal"
            );

            let proposal = writer
                .write_proposal(
                    &representative_snapshot,
                    PromotionEvidence::Recurrence {
                        project_count: cluster_root_count,
                    },
                    now,
                )
                .map_err(|error| CronError::PromotionPass(error.to_string()))?;

            proposals.push(proposal);
        }

        Ok(proposals)
    }

    /// Runs the scope demotion scan (todo #182).
    ///
    /// Reads all `scope='global'` skills from PG, runs the SAME deterministic
    /// project-local identifier check that the intrinsic promotion path uses
    /// (but in the inverse direction: a global skill that DOES reference a
    /// project-local identifier is mis-scoped). For each mis-scoped global skill,
    /// emits a demotion proposal citing the offending identifiers.
    ///
    /// **Deterministic-only**: no LLM vote is required. The cited identifier IS the
    /// evidence that the skill is project-local.
    ///
    /// **Propose-only**: writes a `.pending` artifact via the same
    /// `PromotionProposalWriter` writer; the source skill is never mutated.
    ///
    /// # Errors
    ///
    /// Returns `CronError::PromotionPass` if the PG store query fails or if the
    /// proposal writer fails. Never swallows errors — every failure is surfaced
    /// with a `reason_code` in the message.
    async fn scan_demotions(&self, now: DateTime<Utc>) -> Result<Vec<DemotionProposal>, CronError> {
        let demotion_store = match self.demotion_store.as_ref() {
            Some(store) => store,
            None => {
                info!(
                    "demotion pass: no demotion store configured — pass skipped; \
                     configure SCOPE_DEMOTION_STORE to enable global-skill mis-scope detection"
                );
                return Ok(Vec::new());
            }
        };

        if self.project_identifier_tokens.is_empty() {
            info!(
                "demotion pass: no project-identifier tokens configured — \
                 pass skipped (no tokens means nothing can be flagged as project-local)"
            );
            return Ok(Vec::new());
        }

        let global_skills = demotion_store
            .fetch_all_global_skills()
            .await
            .map_err(|error| {
                CronError::PromotionPass(format!(
                    "demotion pass: PG store query failed \
                     [reason_code=scope_demotion_db_error]: {error}"
                ))
            })?;

        info!(
            global_skill_count = global_skills.len(),
            project_identifier_token_count = self.project_identifier_tokens.len(),
            "demotion pass: scanning global skills for project-local identifier references"
        );

        let token_refs: Vec<&str> = self
            .project_identifier_tokens
            .iter()
            .map(String::as_str)
            .collect();

        let writer = PromotionProposalWriter::new(
            self.promotion_writer_config.global_scope_root.clone(),
            self.promotion_writer_config.pending_directory_name.clone(),
        );

        let mut proposals = Vec::new();

        for skill in &global_skills {
            let skill_text = skill.semantic_text();

            // REUSE the same deterministic check #179 defines — inverse direction:
            // a global skill that DOES contain a project-local identifier is mis-scoped.
            let offending = collect_project_local_identifiers(&skill_text, &token_refs);

            if offending.is_empty() {
                debug!(
                    skill_id = %skill.id,
                    "demotion pass: no project-local identifiers found — skill is correctly scoped"
                );
                continue;
            }

            info!(
                skill_id = %skill.id,
                ?offending,
                "demotion pass: global skill references project-local identifiers — writing demotion proposal"
            );

            let proposal = writer
                .write_demotion_proposal(skill, offending, now)
                .map_err(|error| {
                    CronError::PromotionPass(format!(
                        "demotion pass: failed writing proposal for skill `{}` \
                         [reason_code=scope_demotion_write_failed]: {error}",
                        skill.id
                    ))
                })?;

            proposals.push(proposal);
        }

        Ok(proposals)
    }
}

/// Builds clusters of semantically equivalent skills from embedded project-skill rows.
///
/// Uses greedy union-find: for each pair (i, j) with i < j, if cosine similarity
/// meets the threshold AND the LLM verifier confirms equivalence, skills i and j
/// are merged into the same cluster. The resulting clusters are returned as groups
/// of `ProjectSkillRow`.
///
/// Two skills under the SAME project root may end up in the same cluster, but
/// the caller checks `distinct_roots >= threshold` before emitting a proposal.
///
/// # Errors
///
/// Returns a `String` error if the equivalence verifier fails — surfaced as
/// `CronError::PromotionPass` by the caller.
async fn build_recurrence_clusters(
    embedded_skills: &[(ProjectSkillRow, Vec<f32>)],
    similarity_threshold: f32,
    equivalence_verifier: &dyn LlmEquivalenceVerifier,
) -> Result<Vec<Vec<ProjectSkillRow>>, String> {
    let n = embedded_skills.len();
    // Union-find parent array: parent[i] = cluster representative index.
    let mut parent: Vec<usize> = (0..n).collect();

    let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]]; // path compression
            x = parent[x];
        }
        x
    };

    for i in 0..n {
        for j in (i + 1)..n {
            let (row_i, emb_i) = &embedded_skills[i];
            let (row_j, emb_j) = &embedded_skills[j];

            // Fast cosine check before the expensive LLM call.
            let similarity = cosine_similarity_recurrence(emb_i, emb_j);
            if similarity < similarity_threshold {
                continue;
            }

            let text_i = row_i.semantic_text();
            let text_j = row_j.semantic_text();

            let decision = equivalence_verifier
                .decide_equivalence(&text_i, &text_j)
                .await
                .map_err(|error| {
                    format!(
                        "equivalence verifier failed for skills `{}` and `{}`: {error}",
                        row_i.id, row_j.id
                    )
                })?;

            if decision.equivalent {
                debug!(
                    skill_i = %row_i.id,
                    skill_j = %row_j.id,
                    similarity,
                    rationale = %decision.rationale,
                    "recurrence clustering: merging skills into same cluster"
                );
                // Union the two clusters.
                let root_i = find(&mut parent, i);
                let root_j = find(&mut parent, j);
                if root_i != root_j {
                    parent[root_i] = root_j;
                }
            }
        }
    }

    // Collect members by cluster representative.
    let mut cluster_map: HashMap<usize, Vec<&ProjectSkillRow>> = HashMap::new();
    for (i, skill) in embedded_skills.iter().enumerate().take(n) {
        let root = find(&mut parent, i);
        cluster_map.entry(root).or_default().push(&skill.0);
    }

    Ok(cluster_map
        .into_values()
        .map(|members| members.into_iter().cloned().collect())
        .collect())
}

/// Computes cosine similarity between two embeddings.
///
/// Returns `0.0` for zero-magnitude vectors rather than erroring — a zero-magnitude
/// embedding will simply not meet the similarity threshold and will be excluded
/// from clustering. This is appropriate here (recurrence detection is best-effort
/// over PG data); hard errors are reserved for the writer and PG paths.
fn cosine_similarity_recurrence(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (l, r) in left.iter().zip(right.iter()) {
        dot += l * r;
        left_norm += l * l;
        right_norm += r * r;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm.sqrt() * right_norm.sqrt())
}

/// Converts a `ProjectSkillRow` into a `SkillSnapshot` for use with the proposal writer.
///
/// `source_path` is set to the first source path (if any), matching
/// `SkillSnapshot::from_seeded_skill_projection`'s fallback to `SKILL.md`.
fn project_skill_row_to_snapshot(row: &ProjectSkillRow) -> SkillSnapshot {
    let source_path = row
        .source_paths
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("SKILL.md"));
    SkillSnapshot {
        id: row.id.clone(),
        name: row.name.clone(),
        description: row.description.clone(),
        scope: ScopeType::Project,
        source_path,
        tags: row.tags.clone(),
        subunits: Vec::new(),
        embedding: Vec::new(),
    }
}

// ─── Error ────────────────────────────────────────────────────────────────────

/// Errors that can occur during promotion pass execution or proposal writing.
#[derive(Debug, Error)]
pub enum PromotionError {
    #[error("invalid global scope root: {0}")]
    InvalidGlobalScopeRoot(String),
    #[error("global scope root problem: {0}")]
    ScopeRootInvalid(#[from] MergeError),
    #[error("failed serializing promotion frontmatter: {0}")]
    FrontmatterSerialization(String),
    #[error("failed writing promotion proposal `{path}`: {message}")]
    WriteFailure { path: String, message: String },
}

impl PromotionError {
    /// Maps promotion failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidGlobalScopeRoot(_) => "promotion_invalid_global_scope_root",
            Self::ScopeRootInvalid(_) => "promotion_scope_root_invalid",
            Self::FrontmatterSerialization(_) => "promotion_frontmatter_serialization_failed",
            Self::WriteFailure { .. } => "promotion_pending_write_failed",
        }
    }
}

// ─── Rendering ────────────────────────────────────────────────────────────────

/// Frontmatter payload stored in promotion proposal markdown.
#[derive(Serialize)]
struct PromotionProposalFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    /// Canonical tag list (single source of truth). The markdown body no longer
    /// carries a `tags:` line — the graph-builder reader reads tags here.
    tags: Vec<String>,
    origin: &'a str,
    to_scope: &'a str,
    from_scope: &'a str,
    from_path: String,
    evidence: &'a str,
    created_at: String,
    warning_at: String,
    expires_at: String,
}

fn render_pending_markdown(
    snapshot: &SkillSnapshot,
    evidence: &PromotionEvidence,
    now: DateTime<Utc>,
) -> Result<String, PromotionError> {
    let evidence_str = match evidence {
        PromotionEvidence::Intrinsic => "intrinsic",
        PromotionEvidence::Recurrence { .. } => "recurrence",
    };

    let mut sorted_tags = snapshot.tags.clone();
    sorted_tags.sort_unstable();
    let frontmatter = PromotionProposalFrontmatter {
        name: &snapshot.name,
        description: &snapshot.description,
        tags: sorted_tags,
        origin: "promotion_proposal",
        to_scope: "global",
        from_scope: snapshot.scope.as_str(),
        from_path: snapshot.source_path.display().to_string(),
        evidence: evidence_str,
        created_at: now.to_rfc3339(),
        warning_at: pending_default_warning_at(now).to_rfc3339(),
        expires_at: pending_default_expires_at(now).to_rfc3339(),
    };

    let frontmatter_yaml = serde_yaml::to_string(&frontmatter)
        .map_err(|error| PromotionError::FrontmatterSerialization(error.to_string()))?;

    let frontmatter_yaml = frontmatter_yaml
        .strip_prefix("---\n")
        .unwrap_or(&frontmatter_yaml)
        .to_owned();

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&frontmatter_yaml);
    if !frontmatter_yaml.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("---\n\n");
    body.push_str(&format!("# {}\n\n", snapshot.name));
    body.push_str(&snapshot.description);
    body.push_str("\n\n");

    // Tags live in the frontmatter (single source of truth); the body carries
    // only the title, description, and subunit sections.
    if !snapshot.subunits.is_empty() {
        body.push_str("## Procedures\n");
        for subunit in &snapshot.subunits {
            body.push_str(&format!("- {subunit}\n"));
        }
    }

    Ok(body)
}

/// Builds a deterministic promotion proposal directory name that preserves source ID traceability.
fn proposal_directory_name(skill_id: &str) -> String {
    let id_slug = slugify(skill_id);
    format!("promote--{id_slug}")
}

/// Builds a deterministic demotion proposal directory name that preserves source ID traceability.
///
/// Uses a `demote--` prefix to distinguish demotion artifacts from promotion artifacts
/// in the same pending directory and make the human reviewer's intent clear.
fn demotion_directory_name(skill_id: &str) -> String {
    let id_slug = slugify(skill_id);
    format!("demote--{id_slug}")
}

/// Frontmatter payload stored in demotion proposal markdown.
///
/// Records `from_scope: global` and `to_scope: project` so the human reviewer
/// understands the direction, plus the `offending_identifiers` list that triggered
/// the flag — the cited evidence required by the demotion invariant.
#[derive(Serialize)]
struct DemotionProposalFrontmatter<'a> {
    name: &'a str,
    description: &'a str,
    origin: &'a str,
    from_scope: &'a str,
    to_scope: &'a str,
    from_path: String,
    offending_identifiers: &'a [String],
    created_at: String,
    warning_at: String,
    expires_at: String,
}

fn render_demotion_pending_markdown(
    skill: &GlobalSkillRow,
    offending_identifiers: &[String],
    now: DateTime<Utc>,
) -> Result<String, PromotionError> {
    let from_path = skill
        .source_paths
        .first()
        .map(String::as_str)
        .unwrap_or("")
        .to_owned();

    let frontmatter = DemotionProposalFrontmatter {
        name: &skill.name,
        description: &skill.description,
        origin: "demotion_proposal",
        from_scope: "global",
        to_scope: "project",
        from_path,
        offending_identifiers,
        created_at: now.to_rfc3339(),
        warning_at: pending_default_warning_at(now).to_rfc3339(),
        expires_at: pending_default_expires_at(now).to_rfc3339(),
    };

    let frontmatter_yaml = serde_yaml::to_string(&frontmatter)
        .map_err(|error| PromotionError::FrontmatterSerialization(error.to_string()))?;

    let frontmatter_yaml = frontmatter_yaml
        .strip_prefix("---\n")
        .unwrap_or(&frontmatter_yaml)
        .to_owned();

    let mut body = String::new();
    body.push_str("---\n");
    body.push_str(&frontmatter_yaml);
    if !frontmatter_yaml.ends_with('\n') {
        body.push('\n');
    }
    body.push_str("---\n\n");
    body.push_str(&format!("# {}\n\n", skill.name));
    body.push_str(&skill.description);
    body.push_str("\n\n");

    // Surface the offending identifiers in the body so the reviewer sees them immediately.
    body.push_str("## Demotion Evidence\n\n");
    body.push_str(
        "This global skill references the following project-local identifiers, \
         which indicate it is mis-scoped:\n\n",
    );
    for identifier in offending_identifiers {
        body.push_str(&format!("- `{identifier}`\n"));
    }

    Ok(body)
}

fn slugify(value: &str) -> String {
    let normalized = value
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let slug = normalized
        .split('-')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "promoted-skill".to_owned()
    } else {
        slug
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::Utc;
    use domain::{EmbeddingError, EmbeddingService, ExtractionError};
    use infrastructure::{
        EquivalenceDecision, GeneralityDecision, GlobalSkillRow, LlmEquivalenceVerifier,
        ProjectSkillRow, PromotionRecurrenceError, PromotionRecurrenceStore, ScopeDemotionStore,
        SkillGeneralityVerifier,
    };

    use super::*;
    use crate::merge::SkillSnapshot;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn project_snapshot(id: &str, name: &str, desc: &str) -> SkillSnapshot {
        SkillSnapshot {
            id: id.to_owned(),
            name: name.to_owned(),
            description: desc.to_owned(),
            scope: ScopeType::Project,
            source_path: PathBuf::from(format!("/workspace/project/{id}/SKILL.md")),
            tags: vec!["rust".to_owned()],
            subunits: vec!["do the thing".to_owned()],
            embedding: vec![1.0, 0.0],
        }
    }

    /// Builds a `ProjectSkillRow` with a source path under the given project root.
    fn project_skill_row(id: &str, name: &str, desc: &str, project_root: &str) -> ProjectSkillRow {
        ProjectSkillRow {
            id: id.to_owned(),
            name: name.to_owned(),
            description: desc.to_owned(),
            tags: vec![],
            source_paths: vec![format!("{project_root}/skills/{id}/SKILL.md")],
        }
    }

    /// Mock generality verifier for deterministic unit tests.
    struct MockGeneralityVerifier {
        always_general: bool,
        always_error: bool,
    }

    impl MockGeneralityVerifier {
        fn general() -> Arc<Self> {
            Arc::new(Self {
                always_general: true,
                always_error: false,
            })
        }

        #[allow(dead_code)]
        fn not_general() -> Arc<Self> {
            Arc::new(Self {
                always_general: false,
                always_error: false,
            })
        }

        fn error() -> Arc<Self> {
            Arc::new(Self {
                always_general: false,
                always_error: true,
            })
        }
    }

    #[async_trait]
    impl SkillGeneralityVerifier for MockGeneralityVerifier {
        async fn decide_generality(
            &self,
            _skill_text: &str,
        ) -> Result<GeneralityDecision, ExtractionError> {
            if self.always_error {
                return Err(ExtractionError::ProviderUnavailable(
                    "mock provider unavailable".to_owned(),
                ));
            }
            Ok(GeneralityDecision {
                general: self.always_general,
                rationale: if self.always_general {
                    "mock: general".to_owned()
                } else {
                    "mock: not general".to_owned()
                },
            })
        }
    }

    // ── Recurrence-path mocks ─────────────────────────────────────────────────

    /// Mock recurrence store that returns a fixed set of rows.
    struct MockRecurrenceStore {
        rows: Vec<ProjectSkillRow>,
        fail: bool,
    }

    impl MockRecurrenceStore {
        fn with_rows(rows: Vec<ProjectSkillRow>) -> Arc<Self> {
            Arc::new(Self { rows, fail: false })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                rows: vec![],
                fail: true,
            })
        }
    }

    #[async_trait]
    impl PromotionRecurrenceStore for MockRecurrenceStore {
        async fn fetch_all_project_skills(
            &self,
        ) -> Result<Vec<ProjectSkillRow>, PromotionRecurrenceError> {
            if self.fail {
                return Err(PromotionRecurrenceError::Database(sqlx::Error::Io(
                    std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "mock db failure"),
                )));
            }
            Ok(self.rows.clone())
        }
    }

    /// Mock embedding service: returns identical unit vectors for all inputs
    /// (so cosine similarity is always 1.0 for any pair — clusters everything).
    struct AlwaysSimilarEmbeddingService;

    #[async_trait]
    impl EmbeddingService for AlwaysSimilarEmbeddingService {
        async fn embed_text(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Ok(vec![1.0, 0.0])
        }

        async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
            Ok(texts.iter().map(|_| vec![1.0, 0.0]).collect())
        }
    }

    /// Mock equivalence verifier: always returns equivalent=true.
    struct AlwaysEquivalentVerifier;

    #[async_trait]
    impl LlmEquivalenceVerifier for AlwaysEquivalentVerifier {
        async fn decide_equivalence(
            &self,
            _left_text: &str,
            _right_text: &str,
        ) -> Result<EquivalenceDecision, ExtractionError> {
            Ok(EquivalenceDecision {
                equivalent: true,
                rationale: "mock: always equivalent".to_owned(),
            })
        }
    }

    /// Mock equivalence verifier: always returns equivalent=false.
    #[allow(dead_code)]
    struct NeverEquivalentVerifier;

    #[async_trait]
    impl LlmEquivalenceVerifier for NeverEquivalentVerifier {
        async fn decide_equivalence(
            &self,
            _left_text: &str,
            _right_text: &str,
        ) -> Result<EquivalenceDecision, ExtractionError> {
            Ok(EquivalenceDecision {
                equivalent: false,
                rationale: "mock: never equivalent".to_owned(),
            })
        }
    }

    // ── Demotion-path mocks ───────────────────────────────────────────────────

    /// Builds a `GlobalSkillRow` for use in demotion unit tests.
    fn global_skill_row(id: &str, name: &str, desc: &str) -> GlobalSkillRow {
        GlobalSkillRow {
            id: id.to_owned(),
            name: name.to_owned(),
            description: desc.to_owned(),
            tags: vec![],
            source_paths: vec![format!("/home/user/.claude/skills/{id}/SKILL.md")],
        }
    }

    /// Mock demotion store that returns a fixed set of global skill rows.
    struct MockScopeDemotionStore {
        rows: Vec<GlobalSkillRow>,
        fail: bool,
    }

    impl MockScopeDemotionStore {
        fn with_rows(rows: Vec<GlobalSkillRow>) -> Arc<Self> {
            Arc::new(Self { rows, fail: false })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                rows: vec![],
                fail: true,
            })
        }
    }

    #[async_trait]
    impl ScopeDemotionStore for MockScopeDemotionStore {
        async fn fetch_all_global_skills(
            &self,
        ) -> Result<Vec<GlobalSkillRow>, infrastructure::ScopeDemotionError> {
            if self.fail {
                return Err(infrastructure::ScopeDemotionError::Database(
                    sqlx::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        "mock db failure for demotion store",
                    )),
                ));
            }
            Ok(self.rows.clone())
        }
    }

    // ── Recurrence config unit tests ──────────────────────────────────────────

    /// Proves `RecurrenceConfig::default()` uses min_distinct_roots=2.
    #[test]
    fn recurrence_config_default_threshold_is_two() {
        let config = RecurrenceConfig::default();
        assert_eq!(config.min_distinct_roots, 2);
    }

    /// Proves `cosine_similarity_recurrence` returns 1.0 for identical vectors.
    #[test]
    fn cosine_similarity_identical_vectors_returns_one() {
        let v = vec![1.0_f32, 2.0, 3.0];
        let sim = cosine_similarity_recurrence(&v, &v);
        assert!(
            (sim - 1.0).abs() < 1e-6,
            "identical vectors must have cosine similarity = 1.0, got {sim}"
        );
    }

    /// Proves `cosine_similarity_recurrence` returns 0.0 for orthogonal vectors.
    #[test]
    fn cosine_similarity_orthogonal_vectors_returns_zero() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        let sim = cosine_similarity_recurrence(&a, &b);
        assert!(
            sim.abs() < 1e-6,
            "orthogonal vectors must have cosine similarity = 0.0, got {sim}"
        );
    }

    /// Proves `cosine_similarity_recurrence` returns 0.0 for mismatched dimensions
    /// rather than panicking.
    #[test]
    fn cosine_similarity_mismatched_dims_returns_zero_not_panic() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0, 0.5];
        let sim = cosine_similarity_recurrence(&a, &b);
        assert_eq!(sim, 0.0, "mismatched dimensions must return 0.0, not panic");
    }

    /// Proves `cosine_similarity_recurrence` returns 0.0 for zero-magnitude vectors.
    #[test]
    fn cosine_similarity_zero_magnitude_returns_zero_not_panic() {
        let a = vec![0.0_f32, 0.0];
        let b = vec![1.0_f32, 0.0];
        let sim = cosine_similarity_recurrence(&a, &b);
        assert_eq!(sim, 0.0, "zero-magnitude vector must return 0.0, not panic");
    }

    // ── Recurrence pass unit tests ────────────────────────────────────────────

    /// (AC #2) Skills under TWO distinct project roots cluster and emit a Recurrence
    /// proposal when the LLM confirms equivalence.
    #[tokio::test]
    async fn recurrence_pass_two_distinct_roots_emits_recurrence_proposal() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_recurrence_two_roots_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        // Two equivalent skills from two distinct project roots.
        let rows = vec![
            project_skill_row(
                "skill-musl-a",
                "musl cross-compile",
                "Cross-compiling Rust to musl needs musl-tools",
                "/workspace/project-a",
            ),
            project_skill_row(
                "skill-musl-b",
                "musl cross-compile",
                "Cross-compiling Rust to musl needs musl-tools",
                "/workspace/project-b",
            ),
        ];

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec![],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: Some(MockRecurrenceStore::with_rows(rows)),
            embedding_service: Some(Arc::new(AlwaysSimilarEmbeddingService)),
            equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
            recurrence_config: RecurrenceConfig {
                min_distinct_roots: 2,
                similarity_threshold: 0.5,
            },
            demotion_store: None,
        };

        let proposals = runner
            .run_promotion_pass(Utc::now())
            .await
            .expect("pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            !proposals.is_empty(),
            "two-root cluster must yield at least one Recurrence proposal"
        );
        assert!(
            proposals.iter().any(|p| matches!(
                p.evidence,
                PromotionEvidence::Recurrence { project_count: 2 }
            )),
            "proposal evidence must be Recurrence{{project_count: 2}}, got: {:?}",
            proposals.iter().map(|p| &p.evidence).collect::<Vec<_>>()
        );
    }

    /// (AC #2) Two skills under the SAME project root must NOT produce a Recurrence proposal.
    #[tokio::test]
    async fn recurrence_pass_same_root_does_not_emit_proposal() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_recurrence_same_root_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        // Two skills from the SAME project root.
        let rows = vec![
            project_skill_row(
                "skill-musl-1",
                "musl cross-compile",
                "Cross-compiling Rust to musl needs musl-tools",
                "/workspace/only-project",
            ),
            project_skill_row(
                "skill-musl-2",
                "musl cross-compile",
                "Cross-compiling Rust to musl needs musl-tools",
                "/workspace/only-project",
            ),
        ];

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec![],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: Some(MockRecurrenceStore::with_rows(rows)),
            embedding_service: Some(Arc::new(AlwaysSimilarEmbeddingService)),
            equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
            recurrence_config: RecurrenceConfig {
                min_distinct_roots: 2,
                similarity_threshold: 0.5,
            },
            demotion_store: None,
        };

        let proposals = runner
            .run_promotion_pass(Utc::now())
            .await
            .expect("pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        // Only 1 distinct root → no proposal.
        let recurrence_proposals: Vec<_> = proposals
            .iter()
            .filter(|p| matches!(p.evidence, PromotionEvidence::Recurrence { .. }))
            .collect();
        assert!(
            recurrence_proposals.is_empty(),
            "same-root skills must NOT produce a Recurrence proposal; got: {recurrence_proposals:?}"
        );
    }

    /// (AC #3) Threshold check: only 1 root in DB → no proposals AND threshold-not-met
    /// behavior (verified by pass completing without error and returning no recurrence proposals).
    #[tokio::test]
    async fn recurrence_pass_single_root_threshold_not_met() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_recurrence_single_root_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let rows = vec![project_skill_row(
            "skill-solo",
            "solo skill",
            "Only one project has this",
            "/workspace/solo-project",
        )];

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec![],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: Some(MockRecurrenceStore::with_rows(rows)),
            embedding_service: Some(Arc::new(AlwaysSimilarEmbeddingService)),
            equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
            recurrence_config: RecurrenceConfig {
                min_distinct_roots: 2,
                similarity_threshold: 0.5,
            },
            demotion_store: None,
        };

        let proposals = runner
            .run_promotion_pass(Utc::now())
            .await
            .expect("pass must succeed: single root is not an error");

        let _ = std::fs::remove_dir_all(&global_root);

        let recurrence_proposals: Vec<_> = proposals
            .iter()
            .filter(|p| matches!(p.evidence, PromotionEvidence::Recurrence { .. }))
            .collect();
        assert!(
            recurrence_proposals.is_empty(),
            "single root must produce no Recurrence proposals (threshold not met)"
        );
    }

    /// (AC #5) PG store failure surfaces as CronError::PromotionPass, never swallowed.
    #[tokio::test]
    async fn recurrence_pass_db_failure_surfaces_as_cron_error() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_recurrence_db_fail_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec![],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: Some(MockRecurrenceStore::failing()),
            embedding_service: Some(Arc::new(AlwaysSimilarEmbeddingService)),
            equivalence_verifier: Some(Arc::new(AlwaysEquivalentVerifier)),
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: None,
        };

        let result = runner.run_promotion_pass(Utc::now()).await;

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            result.is_err(),
            "DB failure must propagate as an Err, not a silent skip"
        );
        assert!(
            matches!(result.unwrap_err(), CronError::PromotionPass(_)),
            "error must be CronError::PromotionPass with a reason_code in the message"
        );
    }

    // ── Acceptance criterion #3: deterministic identifier check ───────────────

    /// (AC #3a) An identifier-free skill text returns `false` from the veto check.
    #[test]
    fn identifier_check_passes_for_identifier_free_text() {
        let skill_text =
            "Declare Cargo [[bin]] explicitly or the binary is named after the package";
        let tokens = &["my_project", "special_crate"];
        assert!(
            !skill_text_contains_project_local_identifier(skill_text, tokens),
            "identifier-free text must not be vetoed"
        );
    }

    /// (AC #3b) A skill text containing a project-local token returns `true` (veto).
    #[test]
    fn identifier_check_vetoes_text_containing_project_local_token() {
        let skill_text = "In my_project, always declare the bin explicitly in Cargo.toml to avoid name confusion";
        let tokens = &["my_project", "special_crate"];
        assert!(
            skill_text_contains_project_local_identifier(skill_text, tokens),
            "text containing a project-local token must be vetoed"
        );
    }

    /// Empty tokens slice never triggers a veto.
    #[test]
    fn identifier_check_with_empty_tokens_never_vetoes() {
        let skill_text = "any skill text";
        assert!(
            !skill_text_contains_project_local_identifier(skill_text, &[]),
            "empty token list must never trigger a veto"
        );
    }

    // ── Acceptance criterion #4: path confinement ─────────────────────────────

    /// (AC #4) Writer rejects a global scope root that does not exist.
    #[test]
    fn writer_rejects_nonexistent_global_scope_root() {
        let nonexistent = PathBuf::from("/tmp/does_not_exist_promotion_test_scope_root");
        let writer = PromotionProposalWriter::new(nonexistent, ".skills".to_owned());
        let snapshot = project_snapshot("skill-abc", "cargo bin name", "Declare bin explicitly");
        let result = writer.write_proposal(&snapshot, PromotionEvidence::Intrinsic, Utc::now());
        assert!(
            result.is_err(),
            "writer must fail when global scope root does not exist"
        );
    }

    /// (AC #4) Writer successfully writes within the global scope root when it exists.
    #[test]
    fn writer_writes_within_global_scope_root() {
        let global_root =
            std::env::temp_dir().join(format!("promotion_test_global_root_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir must succeed");

        let writer = PromotionProposalWriter::new(global_root.clone(), ".skills".to_owned());
        let snapshot = project_snapshot("skill-musl", "musl cross-compile", "needs musl-tools");
        let result = writer.write_proposal(&snapshot, PromotionEvidence::Intrinsic, Utc::now());

        let _ = std::fs::remove_dir_all(&global_root);

        let proposal = result.expect("write must succeed");
        // to_scope is always Global.
        assert_eq!(proposal.to_scope, ScopeType::Global);
        // from_scopes carries the source scope.
        assert_eq!(proposal.from_scopes, vec![ScopeType::Project]);
        // evidence is Intrinsic.
        assert_eq!(proposal.evidence, PromotionEvidence::Intrinsic);
    }

    // ── Live runner unit tests ─────────────────────────────────────────────────

    /// (AC #2) Intrinsic-path promotion emits a global `.pending` for an identifier-free
    /// project skill when the LLM says general.
    #[tokio::test]
    async fn live_runner_emits_proposal_for_identifier_free_general_skill() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_runner_test_global_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let snapshot = project_snapshot(
            "skill-cargo-bin",
            "declare cargo bin explicitly",
            "Declare [[bin]] explicitly or binary is named after package",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![snapshot],
            generality_verifier: MockGeneralityVerifier::general(),
            project_identifier_tokens: vec!["my_project".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            // Recurrence path disabled for intrinsic-only unit tests.
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: None,
        };

        let proposals = runner
            .run_promotion_pass(Utc::now())
            .await
            .expect("pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert_eq!(proposals.len(), 1, "one proposal expected");
        assert_eq!(proposals[0].to_scope, ScopeType::Global);
        assert_eq!(proposals[0].evidence, PromotionEvidence::Intrinsic);
        assert_eq!(proposals[0].from_scopes, vec![ScopeType::Project]);
    }

    /// (AC #3b) A project skill naming a project-local identifier is NOT promoted
    /// even if the LLM would say general (deterministic veto wins).
    #[tokio::test]
    async fn live_runner_does_not_promote_skill_with_project_local_identifier() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_runner_veto_test_global_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let snapshot = project_snapshot(
            "skill-myproj-deploy",
            "deploy my_project",
            "In my_project, run deploy.sh with the prod flag to release",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![snapshot],
            generality_verifier: MockGeneralityVerifier::general(), // would say general
            project_identifier_tokens: vec!["my_project".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            // Recurrence path disabled for intrinsic-only unit tests.
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: None,
        };

        let proposals = runner
            .run_promotion_pass(Utc::now())
            .await
            .expect("pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            proposals.is_empty(),
            "no proposals expected: identifier veto must suppress promotion even when LLM says general"
        );
    }

    /// (AC #5) Verifier failure propagates as a pass error, not a silent skip.
    #[tokio::test]
    async fn live_runner_surfaces_verifier_failure_as_cron_error() {
        let global_root = std::env::temp_dir().join(format!(
            "promotion_runner_error_test_global_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let snapshot = project_snapshot(
            "skill-general",
            "cargo bin naming",
            "Declare bin explicitly",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![snapshot],
            generality_verifier: MockGeneralityVerifier::error(),
            project_identifier_tokens: vec![],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            // Recurrence path disabled for intrinsic-only unit tests.
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: None,
        };

        let result = runner.run_promotion_pass(Utc::now()).await;

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            result.is_err(),
            "verifier failure must propagate as CronError, not a silent skip"
        );
        assert!(
            matches!(result.unwrap_err(), CronError::PromotionPass(_)),
            "error must be CronError::PromotionPass"
        );
    }

    // ── Demotion pass unit tests ──────────────────────────────────────────────

    /// (AC #1a) A global skill referencing a project-local identifier produces a
    /// demotion proposal that CITES the offending identifier(s).
    #[tokio::test]
    async fn demotion_pass_misscoped_global_skill_produces_proposal_with_cited_identifiers() {
        let global_root =
            std::env::temp_dir().join(format!("demotion_test_misscoped_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        // A global skill that mentions "dynamic-agent-skill-layer" — a project-local name.
        let misscoped_skill = global_skill_row(
            "skill-dyn-agent-workflow",
            "dynamic-agent-skill-layer workflow",
            "In dynamic-agent-skill-layer, always use the scope promotion pass before retiring skills",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["dynamic-agent-skill-layer".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::with_rows(vec![misscoped_skill])),
        };

        let proposals = runner
            .run_demotion_pass(Utc::now())
            .await
            .expect("demotion pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert_eq!(
            proposals.len(),
            1,
            "one demotion proposal expected for the mis-scoped skill"
        );
        let proposal = &proposals[0];
        assert_eq!(
            proposal.from_scope,
            ScopeType::Global,
            "from_scope must be Global"
        );
        assert_eq!(
            proposal.to_scope,
            ScopeType::Project,
            "to_scope must be Project"
        );
        assert!(
            proposal
                .offending_identifiers
                .contains(&"dynamic-agent-skill-layer".to_owned()),
            "offending_identifiers must cite 'dynamic-agent-skill-layer'; got: {:?}",
            proposal.offending_identifiers
        );
        assert!(
            !proposal.offending_identifiers.is_empty(),
            "demotion proposal must carry at least one offending identifier (cited evidence)"
        );
    }

    /// (AC #1b) A genuinely general global skill (no project-local identifiers) produces
    /// NO demotion proposal.
    #[tokio::test]
    async fn demotion_pass_genuinely_general_global_skill_produces_no_proposal() {
        let global_root =
            std::env::temp_dir().join(format!("demotion_test_general_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        // A truly general global skill — no project-local token.
        let general_skill = global_skill_row(
            "skill-cargo-bin",
            "declare cargo bin explicitly",
            "Declare [[bin]] explicitly in Cargo.toml or the binary is named after the package",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["dynamic-agent-skill-layer".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::with_rows(vec![general_skill])),
        };

        let proposals = runner
            .run_demotion_pass(Utc::now())
            .await
            .expect("demotion pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            proposals.is_empty(),
            "a general global skill must NOT produce a demotion proposal; got: {proposals:?}"
        );
    }

    /// (AC #2) Demotion proposal is a `.pending` file; the source skill is NOT mutated.
    #[tokio::test]
    async fn demotion_proposal_is_pending_file_not_source_mutation() {
        let global_root =
            std::env::temp_dir().join(format!("demotion_test_pending_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let misscoped_skill = global_skill_row(
            "skill-project-specific",
            "myproject deploy",
            "In myproject, run deploy.sh to release",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["myproject".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::with_rows(vec![misscoped_skill])),
        };

        let proposals = runner
            .run_demotion_pass(Utc::now())
            .await
            .expect("demotion pass must succeed");

        assert_eq!(proposals.len(), 1, "one demotion proposal expected");
        let proposal = &proposals[0];

        // The proposal must be a `.pending` file inside the global scope root.
        assert!(
            proposal.pending_path.exists(),
            "demotion proposal must exist as a file at {:?}",
            proposal.pending_path
        );
        assert!(
            proposal.pending_path.starts_with(&global_root),
            "demotion proposal must be confined to the global scope root"
        );
        assert!(
            proposal
                .pending_path
                .file_name()
                .is_some_and(|n| n == PENDING_SKILL_FILE_NAME),
            "demotion proposal must be named PENDING_SKILL_FILE_NAME"
        );

        let _ = std::fs::remove_dir_all(&global_root);
    }

    /// (AC #2) Demotion proposal directory uses the `demote--` prefix to distinguish
    /// it from promotion proposals (`promote--`).
    #[tokio::test]
    async fn demotion_proposal_directory_uses_demote_prefix() {
        let global_root =
            std::env::temp_dir().join(format!("demotion_test_prefix_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let misscoped_skill = global_skill_row(
            "skill-project-deploy",
            "myproject deploy",
            "In myproject, run deploy.sh",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["myproject".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::with_rows(vec![misscoped_skill])),
        };

        let proposals = runner
            .run_demotion_pass(Utc::now())
            .await
            .expect("demotion pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert_eq!(proposals.len(), 1);
        let proposal_dir = proposals[0]
            .pending_path
            .parent()
            .expect("must have parent");
        let dir_name = proposal_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        assert!(
            dir_name.starts_with("demote--"),
            "demotion proposal directory must start with 'demote--', got: {dir_name}"
        );
    }

    /// (AC #3) Demotion proposals are surfaced in `MaintenancePassOutcome.demotion_proposals`.
    #[tokio::test]
    async fn demotion_proposals_surfaced_in_maintenance_outcome() {
        use crate::cron::DemotionPassRunner;
        use chrono::Utc;

        let global_root =
            std::env::temp_dir().join(format!("demotion_test_outcome_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let misscoped_skill = global_skill_row(
            "skill-workflow-project",
            "dynamic-agent-skill-layer workflow",
            "Use promote pass in dynamic-agent-skill-layer before retiring",
        );

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["dynamic-agent-skill-layer".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::with_rows(vec![misscoped_skill])),
        };

        // Directly verify the DemotionPassRunner impl on LivePromotionPassRunner.
        let demotion_proposals = runner
            .run_demotion_pass(Utc::now())
            .await
            .expect("demotion pass must succeed");

        let _ = std::fs::remove_dir_all(&global_root);

        assert_eq!(
            demotion_proposals.len(),
            1,
            "one demotion proposal must be surfaced via run_demotion_pass"
        );
        assert_eq!(
            demotion_proposals[0].from_scope,
            ScopeType::Global,
            "demotion proposal from_scope must be Global"
        );
        assert_eq!(
            demotion_proposals[0].to_scope,
            ScopeType::Project,
            "demotion proposal to_scope must be Project"
        );
    }

    /// (AC #1 error path) PG store failure during demotion surfaces as CronError, never swallowed.
    #[tokio::test]
    async fn demotion_pass_db_failure_surfaces_as_cron_error() {
        let global_root =
            std::env::temp_dir().join(format!("demotion_test_db_fail_{}", std::process::id()));
        std::fs::create_dir_all(&global_root).expect("mkdir");

        let mut runner = LivePromotionPassRunner {
            skill_snapshots: vec![],
            generality_verifier: MockGeneralityVerifier::not_general(),
            project_identifier_tokens: vec!["myproject".to_owned()],
            promotion_writer_config: PromotionWriterConfig {
                global_scope_root: global_root.clone(),
                pending_directory_name: ".skills".to_owned(),
            },
            recurrence_store: None,
            embedding_service: None,
            equivalence_verifier: None,
            recurrence_config: RecurrenceConfig::default(),
            demotion_store: Some(MockScopeDemotionStore::failing()),
        };

        let result = runner.run_demotion_pass(Utc::now()).await;

        let _ = std::fs::remove_dir_all(&global_root);

        assert!(
            result.is_err(),
            "DB failure must propagate as an Err, not a silent skip"
        );
        assert!(
            matches!(result.unwrap_err(), CronError::PromotionPass(_)),
            "error must be CronError::PromotionPass with a reason_code in the message"
        );
    }

    /// `collect_project_local_identifiers` returns all matching tokens (not just bool).
    #[test]
    fn collect_identifiers_returns_all_matching_tokens() {
        let skill_text =
            "In myproject, use the dynamic-agent-skill-layer workflow to promote skills";
        let tokens = &["myproject", "dynamic-agent-skill-layer", "unrelated-token"];
        let found = collect_project_local_identifiers(skill_text, tokens);
        assert!(
            found.contains(&"myproject".to_owned()),
            "must find 'myproject': {found:?}"
        );
        assert!(
            found.contains(&"dynamic-agent-skill-layer".to_owned()),
            "must find 'dynamic-agent-skill-layer': {found:?}"
        );
        assert!(
            !found.contains(&"unrelated-token".to_owned()),
            "must NOT find 'unrelated-token': {found:?}"
        );
    }

    /// `collect_project_local_identifiers` returns empty for identifier-free text.
    #[test]
    fn collect_identifiers_returns_empty_for_identifier_free_text() {
        let skill_text = "Declare [[bin]] explicitly in Cargo.toml";
        let tokens = &["myproject", "dynamic-agent-skill-layer"];
        let found = collect_project_local_identifiers(skill_text, tokens);
        assert!(
            found.is_empty(),
            "identifier-free text must return empty vec; got: {found:?}"
        );
    }

    /// `collect_project_local_identifiers` returns deduplicated, sorted results.
    #[test]
    fn collect_identifiers_deduplicates_and_sorts_results() {
        // skill text mentions the same token twice (should appear only once in output).
        let skill_text = "myproject myproject release";
        let tokens = &["myproject", "atoken"];
        let found = collect_project_local_identifiers(skill_text, tokens);
        assert_eq!(
            found.len(),
            1,
            "duplicate token must be deduped; got: {found:?}"
        );
        assert_eq!(found[0], "myproject");
    }
}
