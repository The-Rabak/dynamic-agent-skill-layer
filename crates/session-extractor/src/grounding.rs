//! Extraction grounding validator (multi-view prompt redesign, design §7).
//!
//! The redesigned extraction prompts allow the LLM to author/abstract procedures
//! (not just transcribe mined tool steps). To keep that honest — consistent with
//! the machine-wide no-fakes / fail-loud mandate — every skill is asked to carry
//! `evidence`: concrete anchors (the exact command, error string, or file) copied
//! from the source transcript. This module checks those anchors against the real
//! transcript and rejects candidates that are demonstrably fabricated.
//!
//! ## Policy (recall-first balance)
//!
//! - **Empty evidence is permitted.** The prompt strongly requests evidence, but
//!   absence is NOT treated as fabrication — rejecting every evidence-less candidate
//!   would gut recall (the orchestrator's central value). The contract/payload
//!   checks already reject content-free shells. (This intentionally softens design
//!   §7's "empty evidence → reject"; see the design doc.)
//! - **Non-empty evidence must have at least one anchor that grounds.** If a
//!   candidate cites evidence and NONE of its anchors ground against the transcript,
//!   the whole evidence set is fabricated and the candidate is dropped (loudly, with
//!   the offending anchors logged) rather than silently kept. Requiring *at least
//!   one* (rather than *all*) anchors to ground tolerates light paraphrase while
//!   still catching wholesale invention.
//!
//! ## Anchor grounding: verbatim OR distinctive-token overlap
//!
//! An anchor grounds if EITHER:
//!   1. its normalized form is a substring of the transcript (verbatim quote), OR
//!   2. a sufficient fraction (`MIN_TOKEN_OVERLAP`) of its *distinctive* tokens
//!      appear in the transcript:
//!        - **code-like tokens** (containing a digit or `_./:`) use substring
//!          `contains` — mid-token matches are intentional for paths, error codes,
//!          and identifiers (`005_x.sql`, `error[E0277]`, `src/lib.rs`).
//!        - **word tokens** (no digit or `_./:`; length ≥ 4) require whole-word
//!          membership in the haystack's whitespace-token set. This prevents
//!          natural-language false positives: `"test"` matching "latest", `"rust"`
//!          matching "frustrated", `"this test runs"` grounding on "the latest run".
//!
//! (2) is essential because frontier models paraphrase: they reconstruct evidence
//! as a summary sentence ("The ticket says `004_x.sql`, but `004_y.sql` already
//! exists … I'll author it as `005`") rather than copying a contiguous span. The
//! distinctive literals in that sentence (`004_y.sql`, `005`, filenames, error
//! codes, identifiers) ARE in the transcript — so token overlap grounds it while a
//! wholly invented anchor (which shares almost no distinctive tokens) still fails.
//! Without (2) the validator silently deleted the highest-value concrete skills
//! from real dev sessions (observed live: 3 of 4 best lessons dropped).
//!
//! The comparison is normalized (lowercased, whitespace-collapsed) without an
//! embedding call — cheap and deterministic.
//!
//! ## Haystack scope
//!
//! Grounding checks against the FULL session event stream — not only conversational
//! turns (UserMessage/AssistantMessage) but also ToolCall (`name`, `input_json`),
//! ToolResult (`output`), and FileEdit (`path`, `operation`) content. Evidence
//! anchors are most commonly exact commands, error strings, or file paths that live
//! in tool events rather than prose turns; a prose-only haystack would silently
//! drop the highest-value concrete skills. See `domain::SessionEvent::grounding_text`
//! for the per-variant projection.
//!
//! ## Normalization: once per session
//!
//! `GroundingContext::new` normalizes the full haystack ONCE per session and also
//! pre-builds a whitespace-token `HashSet` for word-boundary lookups. Both
//! `candidate_is_grounded` and `ungrounded_evidence_anchors` accept a shared
//! `&GroundingContext`, avoiding O(N*K) full-transcript re-normalizations when
//! N candidates and K are dropped with warning logs.

use std::collections::HashSet;

use domain::ExtractedSkillCandidate;

/// Minimum fraction of an anchor's distinctive tokens that must appear in the
/// transcript for token-overlap grounding. 0.5 tolerates heavy paraphrase/reordering
/// while a fully fabricated anchor (≈0 shared distinctive tokens) stays ungrounded.
const MIN_TOKEN_OVERLAP: f64 = 0.5;

/// Pre-normalized session haystack for grounding checks.
///
/// Build once per session via [`GroundingContext::new`] and pass a shared reference
/// to [`candidate_is_grounded`] and [`ungrounded_evidence_anchors`]. This ensures:
/// - The full transcript is normalized at most once per session (not once per
///   candidate + once per dropped candidate's warning log).
/// - The whitespace-token set for whole-word lookups (#262) is built once, not
///   per-anchor.
/// - The caller cannot accidentally pass a pre-normalized string to a function that
///   would normalize it again (the previous `transcript_text: &str` API had that
///   contract asymmetry).
pub struct GroundingContext {
    /// Lowercased, whitespace-collapsed full session haystack.
    normalized_haystack: String,
    /// Whitespace-split tokens of `normalized_haystack` as owned strings, in a
    /// set for O(1) whole-word membership tests.
    haystack_word_set: HashSet<String>,
}

impl GroundingContext {
    /// Normalizes `raw_transcript` (lowercase + whitespace-collapse) and builds
    /// the word-token set for whole-word anchor matching.
    ///
    /// `raw_transcript` should be the concatenated text from ALL session events
    /// relevant to grounding (conversational turns AND tool call/result/file-edit
    /// content). Use `domain::SessionEvent::grounding_text` to project each event.
    pub fn new(raw_transcript: &str) -> Self {
        let normalized_haystack = normalize_for_grounding(raw_transcript);
        let haystack_word_set = normalized_haystack
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        Self {
            normalized_haystack,
            haystack_word_set,
        }
    }
}

/// Normalizes text for grounding comparison: lowercase + whitespace-collapsed.
///
/// Avoids the intermediate `Vec<&str>` by writing directly into a `String` via
/// a join over a `split_whitespace` iterator (no heap allocation for the token list).
fn normalize_for_grounding(text: &str) -> String {
    let mut parts = text.split_whitespace();
    match parts.next() {
        None => String::new(),
        Some(first) => {
            let mut out = String::with_capacity(text.len());
            out.push_str(&first.to_lowercase());
            for part in parts {
                out.push(' ');
                out.push_str(&part.to_lowercase());
            }
            out
        }
    }
}

/// Extracts the "distinctive" tokens of an anchor: those carrying grounding signal.
///
/// A token is distinctive if it is code-like (contains a digit or one of `_./:` —
/// filenames, identifiers, error codes, paths) OR is an alphanumeric word of length
/// ≥ 4 (filters articles/prepositions/punctuation that match any transcript). Tokens
/// are lowercased and stripped of surrounding punctuation/backticks so `` `005`. ``
/// → `005`.
fn distinctive_tokens(anchor: &str) -> Vec<String> {
    anchor
        .split_whitespace()
        .map(|raw| {
            raw.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '_' | '.' | '/' | ':'))
                .to_lowercase()
        })
        .filter(|tok| {
            let code_like = tok
                .chars()
                .any(|c| c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | ':'));
            code_like || tok.chars().count() >= 4
        })
        .collect()
}

/// Returns `true` when a single anchor grounds against the session's grounding context:
/// verbatim substring OR distinctive-token overlap ≥ threshold.
///
/// Token matching is split by token class:
/// - **code-like tokens** (digit or `_./:`): substring `contains` — mid-token
///   matches are intentional (filenames, error codes, SQL slot numbers).
/// - **word tokens** (no digit/`_./:`; length ≥ 4): whole-word membership in the
///   haystack token set — prevents false positives like `"test"` matching "latest".
fn anchor_grounds(anchor: &str, ctx: &GroundingContext) -> bool {
    let needle = normalize_for_grounding(anchor);
    if needle.is_empty() {
        return false;
    }
    if ctx.normalized_haystack.contains(&needle) {
        return true; // verbatim quote
    }
    let tokens = distinctive_tokens(anchor);
    if tokens.is_empty() {
        // No distinctive tokens to overlap on — fall back to the (failed) verbatim
        // check only. Avoids grounding on stopwords alone.
        return false;
    }
    let present = tokens
        .iter()
        .filter(|tok| {
            let is_code_like = tok
                .chars()
                .any(|c| c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | ':'));
            if is_code_like {
                // Paths, error codes, SQL slots: substring match is correct.
                ctx.normalized_haystack.contains(tok.as_str())
            } else {
                // Natural-language words: require whole-word match to avoid
                // "test" grounding on "latest", "rust" grounding on "frustrated".
                ctx.haystack_word_set.contains(tok.as_str())
            }
        })
        .count();
    (present as f64) / (tokens.len() as f64) >= MIN_TOKEN_OVERLAP
}

/// Returns the subset of a candidate's evidence anchors that do NOT ground against
/// the session's grounding context (verbatim or token-overlap). Blank anchors are
/// ignored (neither grounded nor ungrounded). Used for observability and the grounding
/// decision.
///
/// Accepts a pre-built `&GroundingContext` so the transcript is not re-normalized here.
pub fn ungrounded_evidence_anchors<'a>(
    candidate: &'a ExtractedSkillCandidate,
    ctx: &GroundingContext,
) -> Vec<&'a String> {
    candidate
        .evidence
        .iter()
        .filter(|anchor| !anchor.trim().is_empty() && !anchor_grounds(anchor, ctx))
        .collect()
}

/// Returns `true` when a candidate's evidence is grounded enough to keep it.
///
/// - empty evidence → grounded (recall-first; not treated as fabrication)
/// - non-empty evidence → grounded iff at least one non-blank anchor appears in
///   the session's grounding context. If every cited anchor is absent, the candidate
///   is treated as fabricated and is NOT grounded.
///
/// Accepts a pre-built `&GroundingContext` so the transcript is not re-normalized here.
pub fn candidate_is_grounded(candidate: &ExtractedSkillCandidate, ctx: &GroundingContext) -> bool {
    // Collect non-blank anchors.
    let non_blank: Vec<&String> = candidate
        .evidence
        .iter()
        .filter(|a| !a.trim().is_empty())
        .collect();
    if non_blank.is_empty() {
        // No (usable) evidence cited — permitted under the recall-first policy.
        return true;
    }
    non_blank.iter().any(|anchor| anchor_grounds(anchor, ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate_with_evidence(evidence: Vec<&str>) -> ExtractedSkillCandidate {
        ExtractedSkillCandidate {
            name: "skill".to_owned(),
            description: "desc".to_owned(),
            procedures: vec!["step".to_owned()],
            evidence: evidence.into_iter().map(str::to_owned).collect(),
            ..Default::default()
        }
    }

    // ── #264: existing behavioral tests (normalize-once refactor must not change
    // observable grounding outcomes) ────────────────────────────────────────────

    #[test]
    fn empty_evidence_is_grounded() {
        let c = candidate_with_evidence(vec![]);
        let ctx = GroundingContext::new("any transcript text");
        assert!(candidate_is_grounded(&c, &ctx));
    }

    #[test]
    fn blank_only_evidence_is_grounded() {
        let c = candidate_with_evidence(vec!["", "   "]);
        let ctx = GroundingContext::new("any transcript text");
        let ctx_x = GroundingContext::new("x");
        assert!(candidate_is_grounded(&c, &ctx));
        assert!(ungrounded_evidence_anchors(&c, &ctx_x).is_empty());
    }

    #[test]
    fn anchor_present_in_transcript_is_grounded() {
        let c = candidate_with_evidence(vec!["error[E0277]: Mutex<T> cannot be held across await"]);
        let transcript =
            "assistant: I see error[E0277]: Mutex<T>  cannot be held   across await in the build";
        let ctx = GroundingContext::new(transcript);
        assert!(
            candidate_is_grounded(&c, &ctx),
            "normalized anchor must match despite whitespace differences"
        );
        assert!(ungrounded_evidence_anchors(&c, &ctx).is_empty());
    }

    #[test]
    fn fully_fabricated_evidence_is_not_grounded() {
        let c = candidate_with_evidence(vec![
            "error: totally invented message that never happened",
            "ran make deploy-to-mars",
        ]);
        let transcript =
            "user: fix the build\nassistant: replaced std::sync::Mutex with tokio::sync::Mutex";
        let ctx = GroundingContext::new(transcript);
        assert!(
            !candidate_is_grounded(&c, &ctx),
            "a candidate whose every anchor is absent from the transcript is fabricated"
        );
        assert_eq!(ungrounded_evidence_anchors(&c, &ctx).len(), 2);
    }

    #[test]
    fn one_real_anchor_among_paraphrased_keeps_candidate() {
        // Tolerate light paraphrase: one verbatim anchor grounds the candidate even
        // if another anchor is a paraphrase that does not substring-match.
        let c = candidate_with_evidence(vec![
            "error[E0277]: cannot be held across await",
            "switched to the async mutex", // paraphrase, not verbatim
        ]);
        let transcript = "build failed: error[E0277]: cannot be held across await; fixed by using tokio::sync::Mutex";
        let ctx = GroundingContext::new(transcript);
        assert!(
            candidate_is_grounded(&c, &ctx),
            "at least one grounded anchor keeps the candidate"
        );
        // The paraphrased anchor is still reported as ungrounded for observability.
        let ungrounded = ungrounded_evidence_anchors(&c, &ctx);
        assert_eq!(ungrounded, vec![&"switched to the async mutex".to_owned()]);
    }

    #[test]
    fn paraphrased_anchor_with_distinctive_tokens_grounds() {
        // The exact live regression: a single reconstructed-sentence anchor that is
        // NOT a contiguous substring, but whose distinctive literals (the migration
        // filenames + slot number) are all present in the transcript. Must ground.
        let c = candidate_with_evidence(vec![
            "The ticket says `004_skill_source_paths.sql`, but `004_session_logs_status_check.sql` already exists (added after the ticket was written). The next free slot is `005`. I'll author it as `005_skill_source_paths.sql`.",
        ]);
        let transcript = "assistant: the file 004_session_logs_status_check.sql already exists in the migrations dir, so the next free slot is 005 and i will author 005_skill_source_paths.sql for the ticket as written";
        let ctx = GroundingContext::new(transcript);
        assert!(
            candidate_is_grounded(&c, &ctx),
            "paraphrased evidence whose distinctive tokens are present must ground"
        );
        assert!(ungrounded_evidence_anchors(&c, &ctx).is_empty());
    }

    #[test]
    fn fabricated_paraphrase_with_no_shared_tokens_stays_ungrounded() {
        // A reconstructed sentence about something that never happened: its
        // distinctive tokens are absent, so token-overlap must NOT rescue it.
        let c = candidate_with_evidence(vec![
            "We provisioned a Kubernetes cluster on Mars and deployed the quantum scheduler via helm rollout",
        ]);
        let transcript = "assistant: the file 004_session_logs_status_check.sql already exists; next slot is 005";
        let ctx = GroundingContext::new(transcript);
        assert!(
            !candidate_is_grounded(&c, &ctx),
            "an anchor sharing no distinctive tokens with the transcript is fabricated"
        );
        assert_eq!(ungrounded_evidence_anchors(&c, &ctx).len(), 1);
    }

    #[test]
    fn stopword_only_anchor_does_not_ground() {
        // Guard against grounding on filler alone: scattered stopwords
        // (no contiguous phrase, so the verbatim path can't fire) must NOT ground via
        // token overlap, because none of them are distinctive.
        let c = candidate_with_evidence(vec!["it is to be in on at"]);
        let transcript = "on the one hand it was at best a to-do; in any case be that as it may";
        let ctx = GroundingContext::new(transcript);
        assert!(
            !candidate_is_grounded(&c, &ctx),
            "short stopwords (<4 chars, non-code) are not distinctive and must not ground"
        );
    }

    // ── #262: word-boundary false-positive tests ─────────────────────────────

    #[test]
    fn natural_language_word_requires_whole_word_match() {
        // "test run fail" must NOT ground against "the latest runs failed" even though
        // each word is a substring of the transcript: "test" ⊂ "latest", "run" ⊂ "runs",
        // "fail" ⊂ "failed". With the old substring approach those 3/3 tokens would reach
        // the 0.5 threshold and falsely ground the anchor. With whole-word membership
        // none of them are exact tokens, so 0/3 < 0.5 → not grounded.
        let c = candidate_with_evidence(vec!["test run fail"]);
        let transcript = "the latest runs failed";
        let ctx = GroundingContext::new(transcript);
        assert!(
            !candidate_is_grounded(&c, &ctx),
            "word tokens must not ground via substring ('test' ⊂ 'latest', 'run' ⊂ 'runs')"
        );
    }

    #[test]
    fn code_like_tokens_still_ground_via_substring() {
        // Code-like tokens (paths, error codes, SQL slots) keep substring matching —
        // mid-token matches are intentional for these.
        let c = candidate_with_evidence(vec!["005_x.sql"]);
        let transcript = "migrating 005_x.sql to the new schema";
        let ctx = GroundingContext::new(transcript);
        assert!(
            candidate_is_grounded(&c, &ctx),
            "code-like token 005_x.sql must ground via substring"
        );
    }

    #[test]
    fn error_code_grounds_via_substring() {
        // Error codes like "error[E0277]" contain brackets that are stripped during
        // token extraction, but the core code-like substring (digit + brackets) must
        // still ground.
        let c = candidate_with_evidence(vec!["error[E0277] trait not satisfied"]);
        let transcript = "cargo build: error[E0277]: the trait bound is not satisfied";
        let ctx = GroundingContext::new(transcript);
        assert!(
            candidate_is_grounded(&c, &ctx),
            "error code error[E0277] must ground via substring"
        );
    }

    #[test]
    fn word_token_grounds_on_exact_whole_word() {
        // A word token that appears exactly as a whole word in the transcript must
        // still ground. Here "borrow", "moved", and "value" appear as exact whitespace
        // tokens in the normalized transcript and should ground via whole-word lookup.
        let c = candidate_with_evidence(vec!["borrow moved value"]);
        let transcript = "error: the borrow is invalid because value was already moved";
        let ctx = GroundingContext::new(transcript);
        // "borrow", "moved", "value" are all exact whitespace tokens in the transcript.
        // 3/3 = 1.0 ≥ 0.5 → grounded.
        assert!(
            candidate_is_grounded(&c, &ctx),
            "word tokens that are exact transcript words must still ground via whole-word check"
        );
    }

    // ── #259: tool-event grounding tests ─────────────────────────────────────

    #[test]
    fn candidate_grounds_on_tool_result_error_string() {
        // A candidate whose only grounded anchor is an error string that appears
        // exclusively in a ToolResult output must be retained when the grounding
        // haystack includes tool events.
        //
        // This test constructs the haystack from a ToolResult's `output` field
        // (the text the orchestrator now includes via SessionEvent::grounding_text),
        // NOT from conversational turns. The candidate has no prose anchor.
        use domain::SessionEvent;

        let tool_result_output = "error[E0502]: cannot borrow `data` as mutable because it is also borrowed as immutable";

        let events: Vec<SessionEvent> = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "fix the borrow checker error".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 1,
                tool_use_id: "tu_001".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command": "cargo build"}"#.to_owned(),
            },
            SessionEvent::ToolResult {
                index: 2,
                tool_use_id: "tu_001".to_owned(),
                is_error: true,
                exit_code: Some(1),
                output: tool_result_output.to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 3,
                content: "I see the issue. Let me fix the borrow overlap.".to_owned(),
            },
        ];

        // Build the haystack the way the orchestrator does (ALL events via grounding_text).
        let haystack: String = events
            .iter()
            .filter_map(|ev| ev.grounding_text())
            .collect::<Vec<_>>()
            .join("\n");

        let ctx = GroundingContext::new(&haystack);

        // Candidate evidence only cites the error string from the ToolResult — it
        // does NOT appear anywhere in the prose assistant message.
        let c = candidate_with_evidence(vec![
            "error[E0502]: cannot borrow `data` as mutable because it is also borrowed as immutable",
        ]);

        assert!(
            candidate_is_grounded(&c, &ctx),
            "error string from ToolResult output must ground when tool events are in the haystack"
        );
    }

    #[test]
    fn candidate_does_not_ground_on_prose_only_haystack_for_tool_only_evidence() {
        // Negative counterpart: the same candidate must NOT ground when the haystack
        // is built from prose (conversational) turns only — proving the widen matters.
        use domain::SessionEvent;

        let tool_result_output = "error[E0502]: cannot borrow `data` as mutable because it is also borrowed as immutable";

        let events: Vec<SessionEvent> = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "fix the borrow checker error".to_owned(),
            },
            SessionEvent::ToolResult {
                index: 2,
                tool_use_id: "tu_001".to_owned(),
                is_error: true,
                exit_code: Some(1),
                output: tool_result_output.to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 3,
                content: "I see the issue. Let me fix the borrow overlap.".to_owned(),
            },
        ];

        // Build prose-ONLY haystack (old behavior): only UserMessage / AssistantMessage.
        let prose_haystack: String = events
            .iter()
            .filter_map(|ev| ev.as_transcript_entry())
            .map(|e| e.content)
            .collect::<Vec<_>>()
            .join("\n");

        let ctx = GroundingContext::new(&prose_haystack);

        let c = candidate_with_evidence(vec![
            "error[E0502]: cannot borrow `data` as mutable because it is also borrowed as immutable",
        ]);

        assert!(
            !candidate_is_grounded(&c, &ctx),
            "prose-only haystack must not contain tool-result error strings"
        );
    }
}
