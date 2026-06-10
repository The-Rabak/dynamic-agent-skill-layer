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
//!      appear verbatim in the transcript.
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

use domain::ExtractedSkillCandidate;

/// Minimum fraction of an anchor's distinctive tokens that must appear in the
/// transcript for token-overlap grounding. 0.5 tolerates heavy paraphrase/reordering
/// while a fully fabricated anchor (≈0 shared distinctive tokens) stays ungrounded.
const MIN_TOKEN_OVERLAP: f64 = 0.5;

/// Normalizes text for grounding comparison: lowercase + whitespace-collapsed.
fn normalize_for_grounding(text: &str) -> String {
    text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
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
            let code_like = tok.chars().any(|c| c.is_ascii_digit() || matches!(c, '_' | '.' | '/' | ':'));
            code_like || tok.chars().count() >= 4
        })
        .collect()
}

/// Returns `true` when a single anchor grounds against the (already-normalized)
/// transcript haystack: verbatim substring OR distinctive-token overlap ≥ threshold.
fn anchor_grounds(anchor: &str, haystack: &str) -> bool {
    let needle = normalize_for_grounding(anchor);
    if needle.is_empty() {
        return false;
    }
    if haystack.contains(&needle) {
        return true; // verbatim quote
    }
    let tokens = distinctive_tokens(anchor);
    if tokens.is_empty() {
        // No distinctive tokens to overlap on — fall back to the (failed) verbatim
        // check only. Avoids grounding on stopwords alone.
        return false;
    }
    let present = tokens.iter().filter(|t| haystack.contains(t.as_str())).count();
    (present as f64) / (tokens.len() as f64) >= MIN_TOKEN_OVERLAP
}

/// Returns the subset of a candidate's evidence anchors that do NOT ground against
/// the transcript (verbatim or token-overlap). Blank anchors are ignored (neither
/// grounded nor ungrounded). Used for observability and the grounding decision.
pub fn ungrounded_evidence_anchors(
    candidate: &ExtractedSkillCandidate,
    transcript_text: &str,
) -> Vec<String> {
    let haystack = normalize_for_grounding(transcript_text);
    candidate
        .evidence
        .iter()
        .filter(|anchor| !anchor.trim().is_empty() && !anchor_grounds(anchor, &haystack))
        .cloned()
        .collect()
}

/// Returns `true` when a candidate's evidence is grounded enough to keep it.
///
/// - empty evidence → grounded (recall-first; not treated as fabrication)
/// - non-empty evidence → grounded iff at least one non-blank anchor appears in
///   the transcript. If every cited anchor is absent, the candidate is treated as
///   fabricated and is NOT grounded.
pub fn candidate_is_grounded(candidate: &ExtractedSkillCandidate, transcript_text: &str) -> bool {
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
    let haystack = normalize_for_grounding(transcript_text);
    non_blank.iter().any(|anchor| anchor_grounds(anchor, &haystack))
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

    #[test]
    fn empty_evidence_is_grounded() {
        let c = candidate_with_evidence(vec![]);
        assert!(candidate_is_grounded(&c, "any transcript text"));
    }

    #[test]
    fn blank_only_evidence_is_grounded() {
        let c = candidate_with_evidence(vec!["", "   "]);
        assert!(candidate_is_grounded(&c, "any transcript text"));
        assert!(ungrounded_evidence_anchors(&c, "x").is_empty());
    }

    #[test]
    fn anchor_present_in_transcript_is_grounded() {
        let c = candidate_with_evidence(vec!["error[E0277]: Mutex<T> cannot be held across await"]);
        let transcript = "assistant: I see error[E0277]: Mutex<T>  cannot be held   across await in the build";
        assert!(
            candidate_is_grounded(&c, transcript),
            "normalized anchor must match despite whitespace differences"
        );
        assert!(ungrounded_evidence_anchors(&c, transcript).is_empty());
    }

    #[test]
    fn fully_fabricated_evidence_is_not_grounded() {
        let c = candidate_with_evidence(vec![
            "error: totally invented message that never happened",
            "ran make deploy-to-mars",
        ]);
        let transcript = "user: fix the build\nassistant: replaced std::sync::Mutex with tokio::sync::Mutex";
        assert!(
            !candidate_is_grounded(&c, transcript),
            "a candidate whose every anchor is absent from the transcript is fabricated"
        );
        assert_eq!(ungrounded_evidence_anchors(&c, transcript).len(), 2);
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
        assert!(
            candidate_is_grounded(&c, transcript),
            "at least one grounded anchor keeps the candidate"
        );
        // The paraphrased anchor is still reported as ungrounded for observability.
        assert_eq!(
            ungrounded_evidence_anchors(&c, transcript),
            vec!["switched to the async mutex".to_owned()]
        );
    }

    #[test]
    fn paraphrased_anchor_with_distinctive_tokens_grounds() {
        // The exact live regression: a single reconstructed-sentence anchor that is
        // NOT a contiguous substring, but whose distinctive literals (the migration
        // filenames + slot number) are all present in the transcript. Must ground.
        let c = candidate_with_evidence(vec![
            "The ticket says `004_skill_source_paths.sql`, but `004_session_logs_status_check.sql` already exists (added after the ticket was written). The next free slot is `005`. I'll author it as `005_skill_source_paths.sql`.",
        ]);
        let transcript = "assistant: the file 004_session_logs_status_check.sql already exists in the migrations dir, so the next free slot is 005 and I will author 005_skill_source_paths.sql for the ticket as written";
        assert!(
            candidate_is_grounded(&c, transcript),
            "paraphrased evidence whose distinctive tokens are present must ground"
        );
        assert!(ungrounded_evidence_anchors(&c, transcript).is_empty());
    }

    #[test]
    fn fabricated_paraphrase_with_no_shared_tokens_stays_ungrounded() {
        // A reconstructed sentence about something that never happened: its
        // distinctive tokens are absent, so token-overlap must NOT rescue it.
        let c = candidate_with_evidence(vec![
            "We provisioned a Kubernetes cluster on Mars and deployed the quantum scheduler via helm rollout",
        ]);
        let transcript = "assistant: the file 004_session_logs_status_check.sql already exists; next slot is 005";
        assert!(
            !candidate_is_grounded(&c, transcript),
            "an anchor sharing no distinctive tokens with the transcript is fabricated"
        );
        assert_eq!(ungrounded_evidence_anchors(&c, transcript).len(), 1);
    }

    #[test]
    fn stopword_only_anchor_does_not_ground() {
        // Guard against the loosening grounding on filler alone: scattered stopwords
        // (no contiguous phrase, so the verbatim path can't fire) must NOT ground via
        // token overlap, because none of them are distinctive.
        let c = candidate_with_evidence(vec!["it is to be in on at"]);
        let transcript = "on the one hand it was at best a to-do; in any case be that as it may";
        assert!(
            !candidate_is_grounded(&c, transcript),
            "short stopwords (<4 chars, non-code) are not distinctive and must not ground"
        );
    }
}
