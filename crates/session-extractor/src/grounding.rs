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
//!   candidate cites evidence and NONE of its anchors appear in the transcript, the
//!   whole evidence set is fabricated and the candidate is dropped (loudly, with the
//!   offending anchors logged) rather than silently kept. Requiring *at least one*
//!   (rather than *all*) anchors to ground tolerates light paraphrase while still
//!   catching wholesale invention.
//!
//! The comparison is normalized (lowercased, whitespace-collapsed) substring
//! matching — robust to formatting differences without an embedding call.

use domain::ExtractedSkillCandidate;

/// Normalizes text for grounding comparison: lowercase + whitespace-collapsed.
fn normalize_for_grounding(text: &str) -> String {
    text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Returns the subset of a candidate's evidence anchors that do NOT appear in the
/// transcript (after normalization). Blank anchors are ignored (neither grounded
/// nor ungrounded). Used for observability and the grounding decision.
pub fn ungrounded_evidence_anchors(
    candidate: &ExtractedSkillCandidate,
    transcript_text: &str,
) -> Vec<String> {
    let haystack = normalize_for_grounding(transcript_text);
    candidate
        .evidence
        .iter()
        .filter(|anchor| {
            let needle = normalize_for_grounding(anchor);
            !needle.is_empty() && !haystack.contains(&needle)
        })
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
    non_blank
        .iter()
        .any(|anchor| haystack.contains(&normalize_for_grounding(anchor)))
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
}
