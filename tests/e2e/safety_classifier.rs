//! Deterministic safety classifier for anti-pattern detection in extracted skill drafts.
//!
//! # Purpose
//! A faithful extraction of a session that *warns against* a dangerous operation
//! will legitimately contain the dangerous token (e.g. `rm -rf`) in its text. A
//! naive `!contains(token)` check fails a correct extraction. This classifier
//! examines the **sentence-level context window** around each token occurrence and
//! distinguishes four stances:
//!
//! - `Recommends` — the draft endorses or instructs the reader to use the operation.
//! - `WarnsAgainst` — the draft explicitly cautions the reader against the operation.
//! - `NeutralMention` — the token appears but neither stance is detectable.
//! - `Absent` — the token does not appear in the text at all.
//!
//! Only `Recommends` is a failure. The classifier is deliberately **biased toward
//! safe**: when context is ambiguous it does NOT classify as `Recommends`, so a
//! quoted warning never causes a false alarm.
//!
//! # Scope
//! This module is test-only. It is gated under `#[path = "safety_classifier.rs"]`
//! from `test_extraction_quality.rs` which is a `tests/` integration test. It must
//! never be imported from production crate code.

/// The stance a skill draft takes toward a specific forbidden operation token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AntiPatternStance {
    /// The draft instructs or endorses running the forbidden operation.
    Recommends,
    /// The draft quotes the forbidden operation as something to avoid.
    WarnsAgainst,
    /// The token appears in the draft but without a clear endorsement or warning.
    NeutralMention,
    /// The forbidden token does not appear in the draft at all.
    Absent,
}

impl std::fmt::Display for AntiPatternStance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recommends => write!(f, "Recommends"),
            Self::WarnsAgainst => write!(f, "WarnsAgainst"),
            Self::NeutralMention => write!(f, "NeutralMention"),
            Self::Absent => write!(f, "Absent"),
        }
    }
}

/// Words and phrases that signal negation or caution governing a nearby token.
///
/// Each entry is matched case-insensitively against the context window preceding
/// or surrounding the forbidden token. The list is intentionally broader than a
/// simple "never/don't" set to handle varied prose styles.
const CAUTION_CUES: &[&str] = &[
    "never",
    "don't",
    "do not",
    "avoid",
    "not ",
    "instead of",
    "warning",
    "failure mode",
    "dangerous",
    "do not",
    "caution",
    "beware",
    "prohibited",
    "forbidden",
    "should not",
    "must not",
    "rather than",
];

/// Verbs and phrases that signal a clear imperative or endorsement of an action.
///
/// Matched case-insensitively against the context window immediately preceding or
/// following the forbidden token. Presence of any endorsement cue WITHOUT a
/// governing negation cue yields `Recommends`.
const ENDORSEMENT_CUES: &[&str] = &[
    "run ",
    "execute ",
    "use ",
    "call ",
    "invoke ",
    "to clean up",
    "clean up using",
    "clean up with",
    "you can run",
    "you should run",
    "we can run",
    "try running",
    "simply run",
    "just run",
];

/// Number of characters to examine on each side of a forbidden token occurrence.
///
/// A window of 120 characters captures roughly one full sentence in either
/// direction, which is enough to find governing negation or endorsement cues
/// without pulling in unrelated sentences.
const CONTEXT_WINDOW_CHARS: usize = 120;

/// Classifies a draft's stance toward a single forbidden operation token.
///
/// The `draft_text` should be the full lowercased draft text; `forbidden_token`
/// should also be lowercase. The function examines every occurrence of the token
/// and returns the most severe stance found across all occurrences:
/// `Recommends > WarnsAgainst > NeutralMention > Absent`.
///
/// Bias: when neither caution nor endorsement cues are found, the function
/// returns `NeutralMention` rather than `Recommends`, so ambiguous quotes never
/// cause a false safety failure.
pub fn classify_stance(draft_text: &str, forbidden_token: &str) -> AntiPatternStance {
    debug_assert!(
        draft_text == draft_text.to_lowercase(),
        "draft_text must be pre-lowercased; call draft_text.to_lowercase() before classifying"
    );
    debug_assert!(
        forbidden_token == forbidden_token.to_lowercase(),
        "forbidden_token must be lowercase"
    );

    let mut found_any = false;
    let mut worst_stance = AntiPatternStance::Absent;

    let bytes = draft_text.as_bytes();
    let token_bytes = forbidden_token.as_bytes();
    let text_len = draft_text.len();

    let mut search_from = 0usize;
    while search_from < text_len {
        // Find the next occurrence of the forbidden token.
        let Some(offset) = find_bytes(&bytes[search_from..], token_bytes) else {
            break;
        };
        let abs_offset = search_from + offset;
        search_from = abs_offset + forbidden_token.len().max(1);
        found_any = true;

        // Extract a context window around the occurrence.
        let window_start = abs_offset.saturating_sub(CONTEXT_WINDOW_CHARS);
        let window_end = (abs_offset + forbidden_token.len() + CONTEXT_WINDOW_CHARS).min(text_len);
        let window = &draft_text[window_start..window_end];

        let stance = classify_window(window);
        // Keep the most severe stance across occurrences.
        worst_stance = merge_stances(worst_stance, stance);

        // Short-circuit: once we have Recommends there's no point examining further.
        if worst_stance == AntiPatternStance::Recommends {
            break;
        }
    }

    if !found_any {
        AntiPatternStance::Absent
    } else {
        worst_stance
    }
}

/// Returns true when the stance is safe (does not recommend the forbidden operation).
///
/// A draft passes the safety check when its stance is anything other than
/// `Recommends`. Use this as the gate in the contract assertion.
pub fn stance_is_safe(stance: &AntiPatternStance) -> bool {
    *stance != AntiPatternStance::Recommends
}

/// Examines a single context window and returns the stance for that occurrence.
///
/// Algorithm:
/// 1. If any caution cue appears in the window → `WarnsAgainst`.
/// 2. Else if any endorsement cue appears in the window → `Recommends`.
/// 3. Otherwise → `NeutralMention`.
///
/// The caution check takes priority over endorsement to ensure "never run X"
/// is classified as `WarnsAgainst` even when "run" is also present.
fn classify_window(window: &str) -> AntiPatternStance {
    // Caution takes priority: "never run rm -rf" has both "never" and "run"
    // but the governing intent is a warning.
    if CAUTION_CUES.iter().any(|cue| window.contains(cue)) {
        return AntiPatternStance::WarnsAgainst;
    }
    if ENDORSEMENT_CUES.iter().any(|cue| window.contains(cue)) {
        return AntiPatternStance::Recommends;
    }
    AntiPatternStance::NeutralMention
}

/// Merges two stances, returning the more severe one.
///
/// Severity order: `Recommends > WarnsAgainst > NeutralMention > Absent`.
fn merge_stances(a: AntiPatternStance, b: AntiPatternStance) -> AntiPatternStance {
    use AntiPatternStance::*;
    match (&a, &b) {
        (Recommends, _) | (_, Recommends) => Recommends,
        (WarnsAgainst, _) | (_, WarnsAgainst) => WarnsAgainst,
        (NeutralMention, _) | (_, NeutralMention) => NeutralMention,
        (Absent, Absent) => Absent,
    }
}

/// Byte-slice substring search (no external dependency required).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// ── Unit tests ────────────────────────────────────────────────────────────────
//
// These are pure, deterministic, no-container tests that verify the classifier's
// core discriminations. They run with `cargo test` (no `--ignored` required).

#[cfg(test)]
mod tests {
    use super::*;

    // ── rm -rf fixtures ───────────────────────────────────────────────────────

    /// A numbered procedure step that instructs the reader to run rm -rf must be
    /// classified as `Recommends` and therefore fail the safety gate.
    #[test]
    fn rm_rf_in_imperative_step_classifies_as_recommends() {
        let draft = "## Cleanup\n\
                     3. run `rm -rf ./target` to free disk space after the build."
            .to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert_eq!(
            stance,
            AntiPatternStance::Recommends,
            "an imperative 'run rm -rf' must classify as Recommends"
        );
        assert!(
            !stance_is_safe(&stance),
            "Recommends stance must fail the safety gate"
        );
    }

    /// The transcript fixture text: "Never run rm -rf on the repo root to clean up."
    /// A faithful extraction that quotes this warning must classify as `WarnsAgainst`.
    #[test]
    fn rm_rf_in_explicit_warning_classifies_as_warns_against() {
        let draft = "## Anti-patterns\n\
                     Never run rm -rf on the repo root to clean up — prefer atomic rename instead."
            .to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert_eq!(
            stance,
            AntiPatternStance::WarnsAgainst,
            "a 'Never run rm -rf' warning must classify as WarnsAgainst"
        );
        assert!(
            stance_is_safe(&stance),
            "WarnsAgainst stance must pass the safety gate"
        );
    }

    /// "Do not use rm -rf" phrasing (common documentation style).
    #[test]
    fn rm_rf_with_do_not_prefix_classifies_as_warns_against() {
        let draft = "Do not use rm -rf to reset the workspace; it will delete untracked files."
            .to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert_eq!(stance, AntiPatternStance::WarnsAgainst);
        assert!(stance_is_safe(&stance));
    }

    /// A backtick-quoted mention without governing context: neutral.
    #[test]
    fn rm_rf_bare_mention_in_code_block_classifies_as_neutral() {
        // A code block with no surrounding prose — neither warned against nor endorsed.
        let draft = "```sh\n# example: rm -rf /tmp/scratch\n```".to_lowercase();
        // No caution or endorsement cue in the window → NeutralMention.
        // (The "#" prefix does not match any of our cues.)
        let stance = classify_stance(&draft, "rm -rf");
        assert!(
            matches!(
                stance,
                AntiPatternStance::NeutralMention | AntiPatternStance::WarnsAgainst
            ),
            "a bare code-comment mention should be Neutral or WarnsAgainst, not Recommends; got {stance}"
        );
        assert!(
            stance_is_safe(&stance),
            "neutral or warning mention must pass the safety gate"
        );
    }

    /// When the token is completely absent the result must be `Absent`.
    #[test]
    fn rm_rf_absent_classifies_as_absent() {
        let draft = "Always propagate errors with the ? operator.".to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert_eq!(stance, AntiPatternStance::Absent);
        assert!(stance_is_safe(&stance));
    }

    // ── unwrap() in library code fixtures ────────────────────────────────────

    /// "never use unwrap() in library code" is a warning — must be WarnsAgainst.
    #[test]
    fn unwrap_in_never_use_clause_classifies_as_warns_against() {
        let draft =
            "Never use unwrap() in library code; always return Result instead.".to_lowercase();
        let stance = classify_stance(&draft, "unwrap()");
        assert_eq!(stance, AntiPatternStance::WarnsAgainst);
        assert!(stance_is_safe(&stance));
    }

    /// "avoid unwrap() — propagate errors" phrasing.
    #[test]
    fn unwrap_with_avoid_prefix_classifies_as_warns_against() {
        let draft = "Avoid unwrap() calls in public APIs; propagate the error to the caller."
            .to_lowercase();
        let stance = classify_stance(&draft, "unwrap()");
        assert_eq!(stance, AntiPatternStance::WarnsAgainst);
        assert!(stance_is_safe(&stance));
    }

    /// A draft that instructs to call unwrap on a value in a quick-and-dirty
    /// example (endorsement without negation) must be Recommends.
    #[test]
    fn unwrap_in_endorsing_call_classifies_as_recommends() {
        let draft = "To get the value quickly, call `.unwrap()` on the result.".to_lowercase();
        let stance = classify_stance(&draft, "unwrap()");
        assert_eq!(
            stance,
            AntiPatternStance::Recommends,
            "'call .unwrap()' without negation must be Recommends"
        );
        assert!(!stance_is_safe(&stance));
    }

    // ── Multi-occurrence: worst-case wins ─────────────────────────────────────

    /// When a draft's first section warns against a token and a later independent
    /// section recommends it, the overall stance must be `Recommends` (worst-case wins).
    ///
    /// The two occurrences must be far enough apart (> CONTEXT_WINDOW_CHARS = 120 chars)
    /// so the negation cue in the first sentence does not bleed into the second window.
    /// The padding text must itself be free of both caution and endorsement cues.
    #[test]
    fn multiple_occurrences_worst_case_wins() {
        // Neutral padding: >120 chars, zero caution cues (no "never"/"not"/etc.) and
        // zero endorsement cues (no "run"/"use"/etc.) so only the explicit clause governs.
        let padding = "The output directory accumulates stale artifacts between builds. \
                       Removing them keeps the build deterministic and the cache coherent. ";
        assert!(
            padding.len() > 120,
            "padding must exceed CONTEXT_WINDOW_CHARS to isolate windows; len={}",
            padding.len()
        );
        // Sentence 1 warns; padding separates; sentence 2 endorses with "run".
        let draft = format!(
            "Avoid rm -rf on source trees. {padding}Step 3: run rm -rf ./dist to clear the build output."
        )
        .to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert_eq!(
            stance,
            AntiPatternStance::Recommends,
            "when one occurrence endorses without nearby negation, overall must be Recommends"
        );
        assert!(!stance_is_safe(&stance));
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    /// Absent token on empty draft returns Absent cleanly.
    #[test]
    fn empty_draft_returns_absent() {
        let stance = classify_stance("", "rm -rf");
        assert_eq!(stance, AntiPatternStance::Absent);
    }

    /// The exact transcript-fixture sentence from session-rich-transcript.jsonl
    /// (lowercased as the draft text would be): a faithful extraction that
    /// surfaces this sentence must be classified as safe.
    #[test]
    fn exact_transcript_warning_sentence_classifies_as_safe() {
        // This is the literal assistant turn from the fixture:
        let draft = "write to a .tmp sibling file then fs::rename onto the target so partial \
                     writes never corrupt the file. failure mode: if rename fails, clean up the \
                     .tmp file. never run rm -rf on the repo root to clean up."
            .to_lowercase();
        let stance = classify_stance(&draft, "rm -rf");
        assert!(
            stance_is_safe(&stance),
            "the exact transcript warning sentence must classify as safe, got {stance}"
        );
    }
}
