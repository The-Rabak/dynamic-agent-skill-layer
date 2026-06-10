//! Dense embedding view-text builders for the T09 multi-view dense retrieval feature.
//!
//! Each public function produces a bounded text string for a single embedding view,
//! assembled from the T03 structured skill fields. These texts are embedded at
//! snapshot-build time and stored on [`crate::orchestrator::SeededSkill`].
//!
//! # View definitions
//!
//! | View | Fields | Purpose |
//! |------|--------|---------|
//! | `e_task` | `use_when`, `procedures` (subunit headings/bullets), `artifacts`, `tools` | When and how the skill is applied |
//! | `e_needs` | `requires`, `invariants` | Prerequisites and constraints |
//! | `e_negative` | `avoid_when` | Situations where the skill must NOT be applied |
//!
//! # Bounded text discipline
//!
//! Each builder imposes a per-view character cap to honour the embedding-window
//! discipline mandated by the plan (no full-body blob). The cap is
//! `DEFAULT_DENSE_VIEW_CHAR_CAP` (default 4096 chars). Callers must NOT pass arbitrary bodies.
//!
//! # Empty-field handling
//!
//! All fields default to empty `Vec<String>` in the persistence layer when the DB
//! columns are `NULL` (migration 009 pre-populated skills). Empty fields contribute
//! nothing to the assembled text, so the returned string may be empty for pre-T03
//! skills. The embedding provider embeds a short string without error; the resulting
//! vector carries low information and produces low cosine scores, which is correct.
//!
//! # Single source of truth
//!
//! This module is the **only** place field→view mapping is defined. `mcp-server`'s
//! `build_graph_from_pg` calls these functions and must NOT inline equivalent logic.
//! That mirrors how `retrieval::bm25::skill_lexical_document` centralises the BM25
//! field policy.

/// Per-view character cap enforced by every view-text builder.
///
/// Limits the text fed to each embed call so no single view exceeds the model
/// context window. Chosen to be comfortably below the minimum practical embedding
/// window (4096 tokens ≈ ~16 000 chars) while giving each view meaningful signal.
///
/// Override via `DENSE_VIEW_CHAR_CAP` (must parse as `usize`). Silently saturates
/// to `DEFAULT_DENSE_VIEW_CHAR_CAP` when absent or empty; panics fail-loud when
/// present and unparseable.
pub const DEFAULT_DENSE_VIEW_CHAR_CAP: usize = 4096;

/// Returns the active character cap for dense view text, read once per call from
/// the environment. Panics fail-loud when `DENSE_VIEW_CHAR_CAP` is set but not a
/// valid `usize` — no silent fallback.
fn dense_view_char_cap() -> usize {
    match std::env::var("DENSE_VIEW_CHAR_CAP") {
        Ok(raw) if raw.trim().is_empty() => DEFAULT_DENSE_VIEW_CHAR_CAP,
        Ok(raw) => raw.trim().parse().unwrap_or_else(|_| {
            panic!(
                "DENSE_VIEW_CHAR_CAP is set but not a valid usize: {:?}",
                raw
            )
        }),
        Err(_) => DEFAULT_DENSE_VIEW_CHAR_CAP,
    }
}

/// Truncates `text` to at most `max_chars` Unicode scalar values.
fn truncate_to_char_boundary(text: &str, max_chars: usize) -> String {
    text.char_indices()
        .nth(max_chars)
        .map(|(byte_pos, _)| text[..byte_pos].to_owned())
        .unwrap_or_else(|| text.to_owned())
}

/// All skill fields required to build dense embedding views.
///
/// Collect fields here before calling [`build_e_task`], [`build_e_needs`], or
/// [`build_e_negative`]. Using a struct (mirroring [`crate::bm25::SkillLexicalFields`])
/// avoids a large argument list and keeps the assembly explicit and testable.
///
/// Fields mirror the T03 columns added by migration 009. All `Vec<String>` slices
/// may be empty when the DB column is `NULL` for a skill written before T03.
pub struct SkillDenseViewFields<'a> {
    /// Task triggers: situations in which this skill applies (`use_when` column).
    pub use_when: &'a [String],
    /// Anti-patterns / prohibited contexts (`avoid_when` column). Used only for
    /// `e_negative`; excluded from all positive-fusion views.
    pub avoid_when: &'a [String],
    /// File types, protocols, config names (`artifacts` column).
    pub artifacts: &'a [String],
    /// Commands, libraries, frameworks, services (`tools` column).
    pub tools: &'a [String],
    /// Verifier-critical constraints (`invariants` column).
    pub invariants: &'a [String],
    /// Prerequisites assumed by this skill (`requires` column).
    pub requires: &'a [String],
    /// Subunit headings and leading bullets, pre-assembled by the caller.
    ///
    /// This should be the concatenated `title + first_bullet` for each subunit —
    /// NOT the full body — so the view stays bounded. The BM25 path assembles a
    /// similar short form in `build_graph_from_pg`.
    pub subunit_procedure_text: &'a str,
}

/// Builds the `e_task` dense embedding view text.
///
/// Captures **when and how** the skill is applied: task triggers, procedure
/// headings/bullets, artifact types, and tools/libraries. This is the view a query
/// like "use cargo to run tests" would match even when those terms don't appear in
/// `name/description/tags` (`e_summary`).
///
/// Field inclusions: `use_when`, `subunit_procedure_text`, `artifacts`, `tools`.
/// Field exclusions: `avoid_when` (negative signal; never enters positive views),
/// `requires`/`invariants` (prerequisite/constraint signal, belongs to `e_needs`).
///
/// The returned string is capped at [`DEFAULT_DENSE_VIEW_CHAR_CAP`] characters
/// (or the `DENSE_VIEW_CHAR_CAP` env override).
pub fn build_e_task(fields: &SkillDenseViewFields<'_>) -> String {
    let cap = dense_view_char_cap();
    let raw = format!(
        "{} {} {} {}",
        fields.use_when.join(" "),
        fields.subunit_procedure_text,
        fields.artifacts.join(" "),
        fields.tools.join(" "),
    );
    let normalised: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_to_char_boundary(&normalised, cap)
}

/// Builds the `e_needs` dense embedding view text.
///
/// Captures **prerequisites and constraints**: what must be true before the skill
/// applies, and invariants the verifier enforces. Queries like "requires postgres
/// running" or "invariant: transaction committed" match here even when those terms
/// are absent from `e_summary`.
///
/// Field inclusions: `requires`, `invariants`.
/// Field exclusions: all others (they belong to `e_task` or `e_negative`).
///
/// The returned string is capped at [`DEFAULT_DENSE_VIEW_CHAR_CAP`] characters.
pub fn build_e_needs(fields: &SkillDenseViewFields<'_>) -> String {
    let cap = dense_view_char_cap();
    let raw = format!(
        "{} {}",
        fields.requires.join(" "),
        fields.invariants.join(" "),
    );
    let normalised: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_to_char_boundary(&normalised, cap)
}

/// Builds the `e_negative` dense embedding view text.
///
/// Captures **when the skill must NOT be applied**: anti-patterns and prohibited
/// contexts. This view is intentionally EXCLUDED from the positive α fusion
/// (`e_summary` + `e_task` + `e_needs`) because including it would boost skills for
/// queries that match their prohibited contexts — the opposite of the intent.
///
/// The view is built and stored on [`crate::orchestrator::SeededSkill`] for
/// observability and future negative-signal use (e.g. penalising skills whose
/// `avoid_when` matches the query). Do NOT pass it to `fuse_dense_views`.
///
/// Field inclusions: `avoid_when` only.
///
/// The returned string is capped at [`DEFAULT_DENSE_VIEW_CHAR_CAP`] characters.
pub fn build_e_negative(fields: &SkillDenseViewFields<'_>) -> String {
    let cap = dense_view_char_cap();
    let raw = fields.avoid_when.join(" ");
    let normalised: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_to_char_boundary(&normalised, cap)
}

/// Fuses the positive dense views into a single α (`l1_semantic`) score for eq.3.
///
/// Takes the max cosine similarity over {`e_summary`, `e_task`, `e_needs`} —
/// **not** `e_negative`, which is a conflict signal and must never enter the
/// positive fusion.
///
/// Max-over-views is chosen over a weighted average because:
/// - A skill that perfectly matches ANY single view should rank as well as one
///   that mediocrely matches all views.
/// - The calibrated relevance floor (0.48) already gates low-alignment skills;
///   max-pooling means the floor stays meaningful (the best view must clear it).
///
/// # Arguments
/// - `prompt_embedding`: the embedded query vector.
/// - `e_summary_cosine`: cosine(prompt, e_summary) — caller computes this from the
///   existing `seeded_skill.embedding` field, which stays unchanged.
/// - `e_task_embedding`: the `e_task` view embedding from `SeededSkill`.
/// - `e_needs_embedding`: the `e_needs` view embedding from `SeededSkill`.
///
/// Returns the maximum cosine similarity, clamped to `[0.0, 1.0]`.
pub fn fuse_dense_views(
    prompt_embedding: &[f32],
    e_summary_cosine: f32,
    e_task_embedding: &[f32],
    e_needs_embedding: &[f32],
) -> f32 {
    use crate::cosine_rank::cosine_similarity;

    let e_task_cosine = if e_task_embedding.is_empty() {
        0.0
    } else {
        cosine_similarity(prompt_embedding, e_task_embedding).max(0.0)
    };

    let e_needs_cosine = if e_needs_embedding.is_empty() {
        0.0
    } else {
        cosine_similarity(prompt_embedding, e_needs_embedding).max(0.0)
    };

    e_summary_cosine.max(e_task_cosine).max(e_needs_cosine)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(item: &str) -> String {
        item.to_owned()
    }

    // RED #1 — view-text builder: e_task contains use_when and tools terms.
    #[test]
    fn e_task_contains_use_when_and_tools_terms() {
        let use_when = vec![
            s("when deploying to kubernetes"),
            s("when scaling services"),
        ];
        let avoid_when = vec![s("when running locally"), s("when unit testing")];
        let artifacts = vec![s("Dockerfile"), s("helm-chart")];
        let tools = vec![s("kubectl"), s("helm")];
        let invariants: Vec<String> = vec![];
        let requires: Vec<String> = vec![];

        let fields = SkillDenseViewFields {
            use_when: &use_when,
            avoid_when: &avoid_when,
            artifacts: &artifacts,
            tools: &tools,
            invariants: &invariants,
            requires: &requires,
            subunit_procedure_text: "Apply manifests with kubectl apply",
        };
        let text = build_e_task(&fields);
        assert!(
            text.contains("kubernetes"),
            "e_task must include use_when term 'kubernetes': got {:?}",
            text
        );
        assert!(
            text.contains("kubectl"),
            "e_task must include tools term 'kubectl': got {:?}",
            text
        );
        assert!(
            text.contains("helm"),
            "e_task must include tools term 'helm': got {:?}",
            text
        );
        assert!(
            text.contains("Dockerfile"),
            "e_task must include artifacts term 'Dockerfile': got {:?}",
            text
        );
        assert!(
            text.contains("Apply manifests"),
            "e_task must include subunit procedure text: got {:?}",
            text
        );
    }

    // RED #1 — view-text builder: e_task must NOT contain avoid_when (negative signal).
    #[test]
    fn e_task_excludes_avoid_when_terms() {
        let use_when = vec![s("when deploying to kubernetes")];
        let avoid_when = vec![s("when running locally"), s("when unit testing")];
        let artifacts: Vec<String> = vec![];
        let tools: Vec<String> = vec![];
        let invariants: Vec<String> = vec![];
        let requires: Vec<String> = vec![];

        let fields = SkillDenseViewFields {
            use_when: &use_when,
            avoid_when: &avoid_when,
            artifacts: &artifacts,
            tools: &tools,
            invariants: &invariants,
            requires: &requires,
            subunit_procedure_text: "",
        };
        let text = build_e_task(&fields);
        assert!(
            !text.contains("unit testing"),
            "e_task must NOT include avoid_when term 'unit testing': got {:?}",
            text
        );
    }

    // RED #1 — view-text builder: e_needs contains requires and invariants.
    #[test]
    fn e_needs_contains_requires_and_invariants_terms() {
        let use_when = vec![s("when deploying to kubernetes")];
        let avoid_when: Vec<String> = vec![];
        let artifacts: Vec<String> = vec![];
        let tools: Vec<String> = vec![];
        let invariants = vec![s("cluster must be reachable"), s("TLS certificate valid")];
        let requires = vec![s("kubectl installed"), s("helm configured")];

        let fields = SkillDenseViewFields {
            use_when: &use_when,
            avoid_when: &avoid_when,
            artifacts: &artifacts,
            tools: &tools,
            invariants: &invariants,
            requires: &requires,
            subunit_procedure_text: "",
        };
        let text = build_e_needs(&fields);
        assert!(
            text.contains("kubectl installed"),
            "e_needs must include requires term 'kubectl installed': got {:?}",
            text
        );
        assert!(
            text.contains("cluster must be reachable"),
            "e_needs must include invariants term: got {:?}",
            text
        );
    }

    // RED #1 — view-text builder: e_needs must NOT contain tools/use_when terms.
    #[test]
    fn e_needs_excludes_task_and_negative_terms() {
        let use_when = vec![s("when deploying to kubernetes")];
        let avoid_when = vec![s("when unit testing")];
        let artifacts: Vec<String> = vec![];
        let tools = vec![s("kubectl")];
        let invariants = vec![s("cluster must be reachable")];
        let requires = vec![s("helm configured")];

        let fields = SkillDenseViewFields {
            use_when: &use_when,
            avoid_when: &avoid_when,
            artifacts: &artifacts,
            tools: &tools,
            invariants: &invariants,
            requires: &requires,
            subunit_procedure_text: "",
        };
        let text = build_e_needs(&fields);
        assert!(
            !text.contains("kubernetes"),
            "e_needs must NOT include use_when term: got {:?}",
            text
        );
        assert!(
            !text.contains("unit testing"),
            "e_needs must NOT include avoid_when term: got {:?}",
            text
        );
    }

    // RED #1 — view-text builder: e_negative contains avoid_when terms.
    #[test]
    fn e_negative_contains_avoid_when_terms() {
        let use_when: Vec<String> = vec![];
        let avoid_when = vec![s("when running locally"), s("when unit testing")];
        let artifacts: Vec<String> = vec![];
        let tools: Vec<String> = vec![];
        let invariants: Vec<String> = vec![];
        let requires: Vec<String> = vec![];

        let fields = SkillDenseViewFields {
            use_when: &use_when,
            avoid_when: &avoid_when,
            artifacts: &artifacts,
            tools: &tools,
            invariants: &invariants,
            requires: &requires,
            subunit_procedure_text: "",
        };
        let text = build_e_negative(&fields);
        assert!(
            text.contains("unit testing"),
            "e_negative must include avoid_when term: got {:?}",
            text
        );
        assert!(
            text.contains("locally"),
            "e_negative must include avoid_when term: got {:?}",
            text
        );
    }

    // RED #1 — empty fields produce empty text without panic.
    #[test]
    fn empty_fields_produce_empty_text_without_panic() {
        let empty: Vec<String> = vec![];
        let fields = SkillDenseViewFields {
            use_when: &empty,
            avoid_when: &empty,
            artifacts: &empty,
            tools: &empty,
            invariants: &empty,
            requires: &empty,
            subunit_procedure_text: "",
        };
        let e_task = build_e_task(&fields);
        let e_needs = build_e_needs(&fields);
        let e_negative = build_e_negative(&fields);
        assert!(
            e_task.is_empty(),
            "e_task with all-empty fields must be empty: got {:?}",
            e_task
        );
        assert!(
            e_needs.is_empty(),
            "e_needs with all-empty fields must be empty: got {:?}",
            e_needs
        );
        assert!(
            e_negative.is_empty(),
            "e_negative with all-empty fields must be empty: got {:?}",
            e_negative
        );
    }

    // RED #1 — text is bounded: never exceeds the character cap.
    #[test]
    fn view_text_is_bounded_by_char_cap() {
        let long_items: Vec<String> = (0..500).map(|i| format!("item-{i}")).collect();
        let long_subunit = "procedure word ".repeat(1000);
        let fields = SkillDenseViewFields {
            use_when: &long_items,
            avoid_when: &long_items,
            artifacts: &long_items,
            tools: &long_items,
            invariants: &long_items,
            requires: &long_items,
            subunit_procedure_text: &long_subunit,
        };
        let cap = DEFAULT_DENSE_VIEW_CHAR_CAP;
        let e_task_chars = build_e_task(&fields).chars().count();
        let e_needs_chars = build_e_needs(&fields).chars().count();
        let e_negative_chars = build_e_negative(&fields).chars().count();
        assert!(
            e_task_chars <= cap,
            "e_task must be bounded: {} chars > cap {}",
            e_task_chars,
            cap
        );
        assert!(
            e_needs_chars <= cap,
            "e_needs must be bounded: {} chars > cap {}",
            e_needs_chars,
            cap
        );
        assert!(
            e_negative_chars <= cap,
            "e_negative must be bounded: {} chars > cap {}",
            e_negative_chars,
            cap
        );
    }

    // RED #2 (guard) — fuse_dense_views with empty task/needs views returns e_summary cosine.
    // This proves the flag-OFF contract: when view embeddings are empty (not built),
    // fuse_dense_views is equivalent to the plain e_summary cosine.
    #[test]
    fn fuse_dense_views_with_empty_extra_views_returns_summary_cosine() {
        let e_summary_cosine = 0.72_f32;
        let result = fuse_dense_views(
            &[1.0, 0.0, 0.0],
            e_summary_cosine,
            &[], // empty e_task embedding
            &[], // empty e_needs embedding
        );
        assert!(
            (result - e_summary_cosine).abs() < 1e-6,
            "fuse_dense_views with empty extra views must return e_summary_cosine unchanged; \
             got {result}, expected {e_summary_cosine}"
        );
    }

    // RED #2 — fuse_dense_views picks the max view score.
    #[test]
    fn fuse_dense_views_returns_max_over_positive_views() {
        // Prompt: [1,0,0]
        // e_summary cosine = 0.0 (orthogonal)
        // e_task embedding = [1,0,0] → cosine 1.0 (perfect match)
        // e_needs embedding = [0,0,1] → cosine 0.0 (orthogonal)
        let prompt = [1.0_f32, 0.0, 0.0];
        let e_task_emb = [1.0_f32, 0.0, 0.0];
        let e_needs_emb = [0.0_f32, 0.0, 1.0];
        let e_summary_cosine = 0.0_f32;

        let result = fuse_dense_views(&prompt, e_summary_cosine, &e_task_emb, &e_needs_emb);
        assert!(
            (result - 1.0_f32).abs() < 1e-6,
            "fuse_dense_views must return max(0.0, 1.0, 0.0)=1.0; got {result}"
        );
    }

    // RED #2 — e_negative is NOT fused into positive views.
    // Structural test: fuse_dense_views signature has no e_negative parameter.
    // This test documents the deliberate exclusion contract by calling the function
    // and asserting the result is non-negative (smoke test that it compiled correctly).
    #[test]
    fn fuse_dense_views_has_no_e_negative_parameter_structural_contract() {
        let result = fuse_dense_views(&[1.0, 0.0], 0.5, &[0.0, 1.0], &[0.0, 1.0]);
        assert!(result >= 0.0, "result must be non-negative: {result}");
    }
}
