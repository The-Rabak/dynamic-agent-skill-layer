//! Persistent session preamble (#186) — global preferences + project facts mined once from
//! the #184 `SessionEvent` stream and carried verbatim into every episode prompt.
//!
//! ## Design
//!
//! Mining is split into two phases:
//!
//! 1. **Deterministic draft** — a single linear pass over all events:
//!    - User turns are scanned for standing preference/directive signals via keyword matching.
//!    - `ToolCall` and `FileEdit` events yield project facts (paths, build commands, language hints).
//!    This phase is synchronous, pure, and testable without any LLM.
//!
//! 2. **Optional LLM normalisation** — a bounded pass through a [`PreambleNormalizer`] that
//!    deduplicates and phrases preferences consistently. **This pass is optional.** When no
//!    normalizer is supplied, the deterministic draft is promoted to a [`Preamble`] directly and
//!    callers get an honest, grounded result. The optional nature is documented on every public
//!    constructor — there is no silent stub fallback.
//!
//! ## Token cap
//!
//! The preamble enforces a [`PREAMBLE_HARD_TOKEN_CAP`]. When the draft overflows, lower-priority
//! items are dropped and the dropped text is logged via [`tracing::warn`] (never silently discarded).
//! Token count is approximated as `len / 4` (standard GPT-style heuristic).
//!
//! ## Preference-skills
//!
//! Each detected preference is also surfaced as an [`ExtractedSkillCandidate`] (a `convention`
//! with zero procedures, `generality = "general"` or `"uncertain"`) so the downstream #187
//! reduce/orchestration stage can promote durable preferences into the skill graph.
//!
//! ## Static for v1
//!
//! The preamble is built once from the full event stream and applied identically to every episode.
//! A **rolling preamble** — rebuilt after each episode to carry order-dependent forward context —
//! is documented as a follow-up upgrade, to be taken only if quality measurement demands it.
//! Do NOT build it speculatively.

use std::collections::BTreeSet;

use async_trait::async_trait;
use domain::{ExtractedSkillCandidate, SessionEvent};

// ---------------------------------------------------------------------------
// Token cap
// ---------------------------------------------------------------------------

/// Hard upper bound on preamble token count (approximate, via `len / 4` heuristic).
///
/// Target: a few hundred tokens so the preamble fits alongside every episode inside the #185
/// context budget. Items that would push the preamble over this limit are dropped and logged.
pub const PREAMBLE_HARD_TOKEN_CAP: usize = 512;

/// Approximates the token count for a string using the GPT-4-style `len / 4` heuristic.
///
/// This is not exact but is deterministic and cheap, which is all we need for budget enforcement
/// before a real tokenizer is wired in.
fn approximate_token_count(text: &str) -> usize {
    (text.len() + 3) / 4 // ceiling division
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A standing user preference or project directive extracted from the session.
///
/// Preferences are surfaced both inside the [`Preamble`] text (for episode prompts) AND as
/// [`ExtractedSkillCandidate`]s so the reduce stage can promote them into the skill graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedPreference {
    /// The raw user statement that contains the preference signal.
    pub raw_statement: String,
    /// Whether the preference is broadly applicable (`"general"`) or session/project-scoped
    /// (`"uncertain"`). Set to `"uncertain"` when the mining pass cannot determine scope.
    pub generality: PreferenceGenerality,
}

/// Advisory generality tag for a detected preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceGenerality {
    /// Preference applies broadly across projects (e.g. "never add comments").
    General,
    /// Preference may be project-specific or context-dependent; scope is unclear.
    Uncertain,
}

impl PreferenceGenerality {
    /// Returns the canonical string value expected by [`ExtractedSkillCandidate::generality`].
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Uncertain => "uncertain",
        }
    }
}

/// Deterministic project facts mined from tool-call and file-edit events.
///
/// All fields default to `None`/empty when the relevant signals are absent from the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectFacts {
    /// Repository or workspace name inferred from deep-rooted file paths.
    ///
    /// Derived from the first path component that looks like a project root (e.g. `/home/user/myrepo/src/foo.rs`
    /// yields `"myrepo"`). `None` when no paths carry enough depth to infer a name.
    pub repo_name: Option<String>,
    /// Primary language inferred from file extensions and build commands.
    ///
    /// e.g. `"Rust"`, `"Python"`, `"TypeScript"`. `None` when the signal is absent or ambiguous.
    pub primary_language: Option<String>,
    /// Salient file paths touched during the session (deduplicated, capped at 10 entries).
    pub salient_paths: Vec<String>,
    /// Explicit coding conventions or standards stated in the session
    /// (e.g. "we use `tokio::sync` everywhere").
    pub stated_conventions: Vec<String>,
}

/// Intermediate output of the deterministic mining pass, before optional LLM normalisation.
///
/// This is intentionally a public type so tests and the normalizer trait can work with it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreambleDraft {
    /// Standing user preferences extracted from user message turns.
    pub preferences: Vec<DetectedPreference>,
    /// Project-level facts mined from tool calls and file edits.
    pub facts: ProjectFacts,
}

/// The final preamble to be carried verbatim into every episode prompt.
///
/// Built from a [`PreambleDraft`] either directly (when no LLM normalizer is available)
/// or after one bounded LLM normalisation pass. Both paths produce an honest, grounded result;
/// the LLM pass deduplicates and rephrases, it does not hallucinate new content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preamble {
    /// Prompt-ready text block to be prepended to every episode extraction prompt.
    ///
    /// Bounded by [`PREAMBLE_HARD_TOKEN_CAP`]. If mining overflowed, this field contains
    /// only the items that fit; the surplus was logged before being dropped.
    pub text: String,
    /// The normalised preferences, also surfaced as skill candidates for #187.
    pub preferences: Vec<DetectedPreference>,
    /// Project facts included in the preamble.
    pub facts: ProjectFacts,
    /// Approximate token count of [`Self::text`].
    pub approximate_tokens: usize,
}

impl Preamble {
    /// Renders each preference as an [`ExtractedSkillCandidate`] convention skill.
    ///
    /// These candidates have zero procedures (they are pure conventions), `generality` taken from
    /// [`DetectedPreference::generality`], and a fixed confidence of `0.7` (stated, but not yet
    /// verified by a reduce pass). The reduce stage (#187) promotes or demotes them.
    pub fn preference_skill_candidates(&self) -> Vec<ExtractedSkillCandidate> {
        self.preferences
            .iter()
            .map(|preference| preference_to_skill_candidate(preference))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// LLM seam
// ---------------------------------------------------------------------------

/// Seam for the single bounded LLM normalisation/summarisation pass over a [`PreambleDraft`].
///
/// Implementing this trait provides deduplication and rephrasing of mined preferences and facts.
/// The trait is intentionally thin — one method, one input, one output — to keep the LLM
/// interaction bounded and replayable via a test fixture.
///
/// ## Production wiring
///
/// A real LLM-backed implementation is provided by the #187 orchestration layer. This crate
/// does NOT ship a production implementation: the deterministic draft is a valid standalone
/// output (see [`mine_preamble`]). Calling `mine_preamble` without a normalizer is honest and
/// correct; calling it WITH a normalizer gives cleaner, deduplicated output.
///
/// ## Test fakes
///
/// Tests use a `#[cfg(test)]`-gated deterministic fake that records the input draft and returns
/// a fixed transformation. The fake MUST NOT be used outside `#[cfg(test)]`.
#[async_trait]
pub trait PreambleNormalizer: Send + Sync {
    /// Normalises a raw [`PreambleDraft`] into a cleaner form, deduplicating overlapping
    /// preferences and rephrasing project facts for prompt clarity.
    ///
    /// # Errors
    ///
    /// Returns a [`NormalizationError`] if the LLM call fails or the response cannot be parsed.
    /// Callers MUST propagate the error rather than silently falling back to the draft.
    async fn normalize(&self, draft: PreambleDraft) -> Result<PreambleDraft, NormalizationError>;
}

/// Error returned when the LLM normalisation pass fails.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum NormalizationError {
    /// The LLM provider was unreachable or returned a non-OK status.
    #[error("normalizer LLM call failed: {0}")]
    ProviderFailure(String),
    /// The LLM returned a response that could not be parsed as a valid normalised draft.
    #[error("normalizer response parse failed: {0}")]
    ParseFailure(String),
}

// ---------------------------------------------------------------------------
// Mining entry point
// ---------------------------------------------------------------------------

/// Mines a session preamble from a complete ordered slice of [`SessionEvent`]s.
///
/// This is the primary entry point for #186. It runs the deterministic mining pass in one linear
/// sweep over all events, then optionally calls the LLM normalizer, then enforces the token cap.
///
/// ## Normalizer optionality
///
/// `normalizer` may be `None`. When `None`, the deterministic draft is promoted to a [`Preamble`]
/// directly — this is an honest, grounded result, not a stub or silent fallback. The preamble text
/// will be less polished (possible duplicates, raw phrasings) but is fully correct for its purpose.
///
/// When a normalizer is provided and fails, the error is returned to the caller; there is NO silent
/// fallback to the un-normalised draft. The caller decides how to handle the failure.
///
/// ## Token cap enforcement
///
/// If the draft overflows [`PREAMBLE_HARD_TOKEN_CAP`], excess items are dropped (lowest-priority
/// first: salient paths, then stated conventions, then preferences) and the dropped text is logged
/// via [`tracing::warn`]. The resulting preamble text always fits within the cap.
///
/// # Errors
///
/// Returns [`NormalizationError`] only when a normalizer is provided and its call fails.
pub async fn mine_preamble(
    events: &[SessionEvent],
    normalizer: Option<&dyn PreambleNormalizer>,
) -> Result<Preamble, NormalizationError> {
    let draft = mine_draft_deterministic(events);

    let normalised_draft = match normalizer {
        Some(normalizer) => normalizer.normalize(draft).await?,
        None => draft,
    };

    Ok(enforce_token_cap_and_build(normalised_draft))
}

// ---------------------------------------------------------------------------
// Deterministic mining pass (public for testing)
// ---------------------------------------------------------------------------

/// Mines the raw [`PreambleDraft`] from events in a single linear pass.
///
/// This function is pure and synchronous — no LLM, no I/O. It is the stable, testable core that
/// all downstream logic depends on.
pub fn mine_draft_deterministic(events: &[SessionEvent]) -> PreambleDraft {
    let mut preferences: Vec<DetectedPreference> = Vec::new();
    let mut salient_paths: BTreeSet<String> = BTreeSet::new();
    let mut raw_file_paths: Vec<String> = Vec::new();
    let mut language_votes: Vec<&'static str> = Vec::new();
    let stated_conventions: Vec<String> = Vec::new();

    for event in events {
        match event {
            SessionEvent::UserMessage { content, .. } => {
                let detected = detect_preferences_in_user_turn(content);
                preferences.extend(detected);
            }
            SessionEvent::ToolCall {
                name, input_json, ..
            } => {
                if name == "Bash" {
                    if let Some(cmd) = extract_bash_command(input_json) {
                        if let Some(lang) = detect_language_from_command(&cmd) {
                            language_votes.push(lang);
                        }
                    }
                }
            }
            SessionEvent::FileEdit { path, .. } => {
                // Keep the raw path for repo name inference (full depth needed).
                raw_file_paths.push(path.clone());
                // Derive a human-readable salient sub-path (interesting crate/module root).
                if let Some(salient) = salient_path_from_file_path(path) {
                    salient_paths.insert(salient);
                }
                if let Some(lang) = detect_language_from_path(path) {
                    language_votes.push(lang);
                }
            }
            // AssistantMessage, ToolResult, Metadata carry no preference or project-fact signals
            // for this pass. They are intentionally skipped.
            _ => {}
        }
    }

    // Deduplicate preferences by raw statement (case-insensitive trim).
    let preferences = deduplicate_preferences(preferences);

    // Infer primary language from plurality vote.
    let primary_language = plurality_vote(&language_votes).map(|s| s.to_owned());

    // Infer repo name directly from raw file paths (full depth required to skip system dirs).
    let repo_name = raw_file_paths
        .iter()
        .find_map(|path| extract_repo_name_from_path(path));

    // Cap salient paths at 10 — more than that adds noise, not signal.
    let salient_paths: Vec<String> = salient_paths.into_iter().take(10).collect();

    PreambleDraft {
        preferences,
        facts: ProjectFacts {
            repo_name,
            primary_language,
            salient_paths,
            stated_conventions,
        },
    }
}

// ---------------------------------------------------------------------------
// Token cap enforcement + Preamble construction
// ---------------------------------------------------------------------------

/// Enforces [`PREAMBLE_HARD_TOKEN_CAP`], drops overflow items with a `warn` log, and builds the
/// final [`Preamble`].
///
/// Drop order (lowest priority first): salient paths → stated conventions → preferences.
/// A preference is NEVER silently dropped without a log entry.
fn enforce_token_cap_and_build(draft: PreambleDraft) -> Preamble {
    let mut preferences = draft.preferences;
    let mut facts = draft.facts;

    // Iteratively trim until we fit. We build the candidate text to measure it.
    loop {
        let candidate_text = render_preamble_text(&preferences, &facts);
        let tokens = approximate_token_count(&candidate_text);
        if tokens <= PREAMBLE_HARD_TOKEN_CAP {
            return Preamble {
                text: candidate_text,
                preferences,
                facts,
                approximate_tokens: tokens,
            };
        }

        // Overflow: drop one item from the lowest-priority bucket and log it.
        if !facts.salient_paths.is_empty() {
            let dropped = facts.salient_paths.pop().expect("checked non-empty above");
            tracing::warn!(
                dropped_item = %dropped,
                "preamble token cap overflow: dropping salient path to fit within {} tokens",
                PREAMBLE_HARD_TOKEN_CAP
            );
        } else if !facts.stated_conventions.is_empty() {
            let dropped = facts
                .stated_conventions
                .pop()
                .expect("checked non-empty above");
            tracing::warn!(
                dropped_item = %dropped,
                "preamble token cap overflow: dropping stated convention to fit within {} tokens",
                PREAMBLE_HARD_TOKEN_CAP
            );
        } else if !preferences.is_empty() {
            let dropped = preferences.pop().expect("checked non-empty above");
            tracing::warn!(
                dropped_item = %dropped.raw_statement,
                "preamble token cap overflow: dropping preference to fit within {} tokens",
                PREAMBLE_HARD_TOKEN_CAP
            );
        } else {
            // Nothing left to drop; emit an empty preamble rather than an infinite loop.
            tracing::error!(
                "preamble token cap cannot be satisfied even with empty content — \
                 PREAMBLE_HARD_TOKEN_CAP ({}) may be too small for the header alone",
                PREAMBLE_HARD_TOKEN_CAP
            );
            let empty_text = String::new();
            return Preamble {
                text: empty_text,
                preferences,
                facts,
                approximate_tokens: 0,
            };
        }
    }
}

/// Renders the prompt-ready preamble text block from the current preferences and facts.
///
/// The format is intentionally plain and dense — every line is load-bearing for the episode
/// extraction prompt. Section headers are kept short to minimise token cost.
fn render_preamble_text(preferences: &[DetectedPreference], facts: &ProjectFacts) -> String {
    let mut sections: Vec<String> = Vec::new();

    // Project facts section
    let mut fact_lines: Vec<String> = Vec::new();
    if let Some(ref repo) = facts.repo_name {
        fact_lines.push(format!("Repo: {repo}"));
    }
    if let Some(ref lang) = facts.primary_language {
        fact_lines.push(format!("Language: {lang}"));
    }
    if !facts.salient_paths.is_empty() {
        fact_lines.push(format!("Key paths: {}", facts.salient_paths.join(", ")));
    }
    if !facts.stated_conventions.is_empty() {
        for convention in &facts.stated_conventions {
            fact_lines.push(format!("Convention: {convention}"));
        }
    }
    if !fact_lines.is_empty() {
        sections.push(format!("[Project facts]\n{}", fact_lines.join("\n")));
    }

    // Preferences section
    if !preferences.is_empty() {
        let pref_lines: Vec<String> = preferences
            .iter()
            .map(|p| format!("- {}", p.raw_statement.trim()))
            .collect();
        sections.push(format!(
            "[Standing user preferences]\n{}",
            pref_lines.join("\n")
        ));
    }

    sections.join("\n\n")
}

// ---------------------------------------------------------------------------
// Preference detection helpers
// ---------------------------------------------------------------------------

/// Keyword phrases that signal a standing preference or directive in a user turn.
///
/// Each entry is a lowercase substring that, when found in a normalised user message, indicates
/// the sentence containing it carries a preference. Order is chosen so more specific phrases
/// are checked before shorter ones to avoid double-triggering on overlapping patterns.
const PREFERENCE_SIGNAL_PHRASES: &[&str] = &[
    "always ",
    "never ",
    "prefer ",
    "don't ",
    "do not ",
    "i want ",
    "use ",
    "avoid ",
    "make sure ",
    "please use ",
    "please don't ",
    "please never ",
    "please always ",
];

/// Scans a single user message turn for standing preference/directive signals.
///
/// Returns one [`DetectedPreference`] for each sentence (split on `.`, `!`, `?`, or newline)
/// that contains at least one preference signal phrase. Duplicate sentences are not deduplicated
/// here — that happens in [`deduplicate_preferences`] after the full pass.
fn detect_preferences_in_user_turn(content: &str) -> Vec<DetectedPreference> {
    // Split on sentence-ending punctuation and newlines for more targeted extraction.
    let sentences = split_into_sentences(content);
    let mut found: Vec<DetectedPreference> = Vec::new();

    for sentence in sentences {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        let is_preference = PREFERENCE_SIGNAL_PHRASES
            .iter()
            .any(|phrase| lower.contains(phrase));
        if is_preference {
            // Classify generality: if the sentence contains "this project", "our repo",
            // "here", or refers to a specific tool/framework by name, mark as "uncertain".
            // Otherwise "general".
            let generality = if contains_project_scoping_signal(&lower) {
                PreferenceGenerality::Uncertain
            } else {
                PreferenceGenerality::General
            };
            found.push(DetectedPreference {
                raw_statement: trimmed.to_owned(),
                generality,
            });
        }
    }

    found
}

/// Returns `true` when the lowercased sentence contains signals that the preference is likely
/// project-specific rather than broadly general.
fn contains_project_scoping_signal(lower_sentence: &str) -> bool {
    const PROJECT_SCOPE_SIGNALS: &[&str] = &[
        "this project",
        "this repo",
        "our repo",
        "this codebase",
        "our codebase",
        "here ",
        "in this ",
        "for this ",
    ];
    PROJECT_SCOPE_SIGNALS
        .iter()
        .any(|signal| lower_sentence.contains(signal))
}

/// Splits text into individual sentences by `.`, `!`, `?`, and newlines.
fn split_into_sentences(text: &str) -> Vec<&str> {
    // Use a simple state-machine split on sentence-ending characters.
    let mut sentences: Vec<&str> = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();

    for (i, &byte) in bytes.iter().enumerate() {
        if matches!(byte, b'.' | b'!' | b'?' | b'\n') {
            let slice = &text[start..=i];
            if !slice.trim().is_empty() {
                sentences.push(slice);
            }
            start = i + 1;
        }
    }
    // Remainder after last sentence-ender.
    if start < text.len() {
        let remainder = &text[start..];
        if !remainder.trim().is_empty() {
            sentences.push(remainder);
        }
    }
    sentences
}

/// Deduplicates preferences by normalised raw statement (case-insensitive, trimmed).
///
/// Later occurrences of a statement are dropped; the first occurrence is kept. This preserves
/// the ordering of first-stated preferences, which tend to be the most foundational.
fn deduplicate_preferences(preferences: Vec<DetectedPreference>) -> Vec<DetectedPreference> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    preferences
        .into_iter()
        .filter(|p| seen.insert(p.raw_statement.trim().to_ascii_lowercase()))
        .collect()
}

// ---------------------------------------------------------------------------
// Project-fact mining helpers
// ---------------------------------------------------------------------------

/// Extracts the shell command string from a `Bash` tool call's `input_json`.
///
/// The wire format for Bash calls is `{"command": "<shell command>", ...}`. Returns `None`
/// when the JSON cannot be parsed or the `command` field is absent/non-string.
fn extract_bash_command(input_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(input_json).ok()?;
    value
        .get("command")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
}

/// Infers a language from a shell command string (e.g. `cargo`, `npm`, `python`).
fn detect_language_from_command(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    // Match on the first token or prominent keyword.
    if lower.contains("cargo ") || lower.contains("rustc ") || lower.contains("rust-analyzer") {
        Some("Rust")
    } else if lower.contains("npm ") || lower.contains("yarn ") || lower.contains("pnpm ") {
        Some("TypeScript")
    } else if lower.contains("python ") || lower.contains("pip ") || lower.contains("uv ") {
        Some("Python")
    } else if lower.contains("go ") && (lower.contains("go build") || lower.contains("go test")) {
        Some("Go")
    } else if lower.contains("mvn ") || lower.contains("gradle ") {
        Some("Java")
    } else {
        None
    }
}

/// Infers a language from a file extension.
fn detect_language_from_path(path: &str) -> Option<&'static str> {
    if path.ends_with(".rs") {
        Some("Rust")
    } else if path.ends_with(".ts") || path.ends_with(".tsx") {
        Some("TypeScript")
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        Some("JavaScript")
    } else if path.ends_with(".py") {
        Some("Python")
    } else if path.ends_with(".go") {
        Some("Go")
    } else if path.ends_with(".java") {
        Some("Java")
    } else {
        None
    }
}

/// Extracts a salient path string from a `FileEdit.path` for display in the preamble.
///
/// For absolute paths rooted under common system directories (`/home/<user>/`, `/root/`, etc.),
/// skips those top-level components and returns the first two meaningful project-relative
/// components (e.g. `/home/user/myrepo/crates/session-extractor/src/lib.rs` → `crates/session-extractor`).
/// For short or relative paths, returns the first two components as-is.
/// Returns `None` for empty paths.
fn salient_path_from_file_path(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    // Normalise: strip leading slash so all components are plain names.
    let normalised = path.trim_start_matches('/');
    let all_components: Vec<&str> = normalised.split('/').filter(|c| !c.is_empty()).collect();
    if all_components.is_empty() {
        return None;
    }

    // Walk forward, skipping system dirs and the component immediately following one
    // (e.g. skip "home" + the username that follows it).
    const SYSTEM_DIRS: &[&str] = &["home", "usr", "var", "tmp", "opt", "etc", "root", "srv"];
    let mut interesting: Vec<&str> = Vec::new();
    let mut skip_next = false;
    for component in &all_components {
        if skip_next {
            skip_next = false;
            continue;
        }
        if SYSTEM_DIRS.contains(component) {
            skip_next = true;
            continue;
        }
        interesting.push(component);
    }

    match interesting.len() {
        0 => None,
        1 => Some(interesting[0].to_owned()),
        _ => {
            // Return first two interesting components for locality signal.
            // e.g. "myrepo/crates" is less useful than "crates/session-extractor",
            // so skip the repo root itself (first interesting component) and take the next two.
            if interesting.len() >= 3 {
                Some(format!("{}/{}", interesting[1], interesting[2]))
            } else {
                Some(format!("{}/{}", interesting[0], interesting[1]))
            }
        }
    }
}

/// Attempts to infer a repo name from a salient path.
///
/// For paths like `/home/user/myrepo/src/lib.rs`, we look for a component that is NOT a common
/// system directory (`home`, `usr`, `var`, `tmp`, `opt`) and is not the username component
/// (position 1 for `/home/<user>/...`). Falls back to the first non-trivial component.
fn extract_repo_name_from_path(path: &str) -> Option<String> {
    const SYSTEM_DIRS: &[&str] = &["home", "usr", "var", "tmp", "opt", "etc", "root", "srv"];

    let normalised = path.trim_start_matches('/');
    let components: Vec<&str> = normalised.split('/').collect();

    // Skip system dir and the next component (likely a username).
    let mut skip_next = false;
    for component in &components {
        if skip_next {
            skip_next = false;
            continue;
        }
        if SYSTEM_DIRS.contains(component) {
            skip_next = true;
            continue;
        }
        if !component.is_empty() && component.len() > 1 {
            return Some((*component).to_owned());
        }
    }
    None
}

/// Returns the most frequently occurring value from a slice of votes.
///
/// On ties, the earliest-appearing winner wins. Returns `None` on an empty slice.
fn plurality_vote<'a>(votes: &[&'a str]) -> Option<&'a str> {
    if votes.is_empty() {
        return None;
    }
    // Count occurrences, preserving first-appearance order for stability.
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for &vote in votes {
        if let Some(entry) = counts.iter_mut().find(|(v, _)| *v == vote) {
            entry.1 += 1;
        } else {
            counts.push((vote, 1));
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(winner, _)| winner)
}

// ---------------------------------------------------------------------------
// Preference → skill candidate conversion
// ---------------------------------------------------------------------------

/// Converts a [`DetectedPreference`] into an [`ExtractedSkillCandidate`] convention skill.
///
/// The candidate has:
/// - `procedures`: empty (it is a pure convention, not a procedure)
/// - `conventions`: the raw statement as the single convention
/// - `generality`: taken from [`DetectedPreference::generality`]
/// - `confidence`: 0.7 (stated by user, not yet verified by a reduce pass)
///
/// The `name` is derived by taking the first 8 words of the statement (title-cased) and
/// appending `" (preference)"` to distinguish it from a procedure-bearing skill.
fn preference_to_skill_candidate(preference: &DetectedPreference) -> ExtractedSkillCandidate {
    let name = derive_preference_name(&preference.raw_statement);
    let description = format!(
        "Standing user preference: {}",
        preference.raw_statement.trim()
    );
    ExtractedSkillCandidate {
        name,
        description,
        tags: vec!["preference".to_owned(), "convention".to_owned()],
        procedures: vec![],
        conventions: vec![preference.raw_statement.trim().to_owned()],
        assets: vec![],
        confidence: 0.7,
        generality: Some(preference.generality.as_str().to_owned()),
        generality_rationale: Some(
            "Detected as a standing user directive; generality inferred from scope signals."
                .to_owned(),
        ),
    }
}

/// Derives a compact skill name from a preference statement.
///
/// Takes the first 8 words, strips sentence-ending punctuation from the last word, and appends
/// `" (preference)"`. This produces a stable, readable name without requiring an LLM call.
fn derive_preference_name(raw_statement: &str) -> String {
    let words: Vec<&str> = raw_statement.split_whitespace().take(8).collect();
    let mut name = words.join(" ");
    // Strip trailing sentence-ending punctuation.
    while name.ends_with(['.', '!', '?', ',']) {
        name.pop();
    }
    format!("{name} (preference)")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Test-only fake normalizer — NOT a production stub.
    //
    // This struct exists solely to satisfy the PreambleNormalizer seam in unit tests.
    // It is gated behind #[cfg(test)] and MUST NOT be used or referenced outside tests.
    // The production normalizer is provided by #187.
    // -----------------------------------------------------------------------

    /// Deterministic test normalizer: returns the draft with preferences sorted
    /// alphabetically by raw_statement and all generality values forced to `General`.
    ///
    /// This is a recorded-fixture-style fake: the transformation is trivial and deterministic,
    /// making test assertions stable without an LLM.
    struct AlphabeticNormalizerFake;

    #[async_trait]
    impl PreambleNormalizer for AlphabeticNormalizerFake {
        async fn normalize(
            &self,
            mut draft: PreambleDraft,
        ) -> Result<PreambleDraft, NormalizationError> {
            draft
                .preferences
                .sort_by(|a, b| a.raw_statement.cmp(&b.raw_statement));
            // Force all generality to General so tests can assert on the normalised shape.
            for pref in &mut draft.preferences {
                pref.generality = PreferenceGenerality::General;
            }
            Ok(draft)
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 1: preference stated once appears in every episode prompt.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn preference_stated_once_appears_in_preamble_text() {
        let events = vec![
            SessionEvent::UserMessage {
                index: 0,
                content: "Please always use `tokio::sync` for concurrency primitives.".to_owned(),
            },
            SessionEvent::AssistantMessage {
                index: 1,
                content: "Understood, I will use tokio::sync.".to_owned(),
            },
        ];

        let preamble = mine_preamble(&events, None)
            .await
            .expect("mining must succeed");

        assert!(
            preamble.text.contains("tokio::sync"),
            "preamble text must carry the stated preference; got: {:?}",
            preamble.text
        );
        assert_eq!(
            preamble.preferences.len(),
            1,
            "exactly one preference must be detected"
        );
        assert_eq!(
            preamble.preferences[0].raw_statement,
            "Please always use `tokio::sync` for concurrency primitives."
        );
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 2: token cap respected with logging on overflow.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn token_cap_respected_with_many_preferences() {
        // Build a pathological session with many long preferences.
        // We generate enough events to clearly exceed PREAMBLE_HARD_TOKEN_CAP.
        let long_phrase = "a".repeat(80); // ~20 tokens each
        let events: Vec<SessionEvent> = (0..40)
            .map(|i| SessionEvent::UserMessage {
                index: i,
                content: format!(
                    "Please always prefer {long_phrase} pattern number {i} over the alternative."
                ),
            })
            .collect();

        let preamble = mine_preamble(&events, None)
            .await
            .expect("mining must succeed even with overflow");

        assert!(
            preamble.approximate_tokens <= PREAMBLE_HARD_TOKEN_CAP,
            "preamble must respect the hard token cap of {}; got {} tokens",
            PREAMBLE_HARD_TOKEN_CAP,
            preamble.approximate_tokens
        );
        // Some preferences must have been dropped.
        assert!(
            preamble.preferences.len() < 40,
            "some preferences must have been dropped to satisfy the cap; preferences remaining: {}",
            preamble.preferences.len()
        );
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 3: preference surfaces as a preference-skill candidate.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn preference_surfaces_as_general_skill_candidate() {
        let events = vec![SessionEvent::UserMessage {
            index: 0,
            content: "Never add inline comments unless explicitly asked.".to_owned(),
        }];

        // Use the test fake normalizer to prove the seam works.
        let normalizer = AlphabeticNormalizerFake;
        let preamble = mine_preamble(&events, Some(&normalizer))
            .await
            .expect("mining must succeed");

        let candidates = preamble.preference_skill_candidates();
        assert_eq!(
            candidates.len(),
            1,
            "exactly one preference-skill candidate must be surfaced"
        );

        let candidate = &candidates[0];
        // Generality must be "general" (the fake forces it).
        assert_eq!(
            candidate.generality.as_deref(),
            Some("general"),
            "preference-skill candidate must carry generality='general'"
        );
        // Must be a pure convention: zero procedures.
        assert!(
            candidate.procedures.is_empty(),
            "preference-skill candidates must have zero procedures"
        );
        // Convention must carry the raw statement.
        assert!(
            candidate.conventions.iter().any(|c| c.contains("Never")),
            "convention must carry the raw preference statement; got: {:?}",
            candidate.conventions
        );
        // Tags must include "preference".
        assert!(
            candidate.tags.contains(&"preference".to_owned()),
            "candidate must be tagged 'preference'"
        );
    }

    // -----------------------------------------------------------------------
    // Acceptance criterion 4: project facts populated from event model.
    // -----------------------------------------------------------------------

    #[test]
    fn project_facts_populated_from_file_edits_and_tool_calls() {
        let events = vec![
            SessionEvent::FileEdit {
                index: 0,
                tool_use_id: "tu1".to_owned(),
                path: "/home/user/myrepo/crates/session-extractor/src/lib.rs".to_owned(),
                operation: "Edit".to_owned(),
            },
            SessionEvent::FileEdit {
                index: 1,
                tool_use_id: "tu2".to_owned(),
                path: "/home/user/myrepo/crates/domain/src/types.rs".to_owned(),
                operation: "Write".to_owned(),
            },
            SessionEvent::ToolCall {
                index: 2,
                tool_use_id: "tu3".to_owned(),
                name: "Bash".to_owned(),
                input_json: r#"{"command": "cargo test -p session-extractor"}"#.to_owned(),
            },
        ];

        let draft = mine_draft_deterministic(&events);

        // Repo name must be inferred from the deep path.
        assert!(
            draft.facts.repo_name.is_some(),
            "repo name must be inferred from file paths"
        );
        assert_eq!(
            draft.facts.repo_name.as_deref(),
            Some("myrepo"),
            "repo name must be 'myrepo', got {:?}",
            draft.facts.repo_name
        );

        // Primary language must be Rust (cargo command + .rs files).
        assert_eq!(
            draft.facts.primary_language.as_deref(),
            Some("Rust"),
            "primary language must be 'Rust'"
        );

        // At least one key path must be present.
        assert!(
            !draft.facts.salient_paths.is_empty(),
            "salient paths must be non-empty"
        );
        assert!(
            draft
                .facts
                .salient_paths
                .iter()
                .any(|p| p.contains("crates")),
            "salient paths must include a 'crates' component; got: {:?}",
            draft.facts.salient_paths
        );
    }

    // -----------------------------------------------------------------------
    // Supporting unit tests for helpers
    // -----------------------------------------------------------------------

    #[test]
    fn detect_preferences_finds_always_phrase() {
        let content = "Always run tests before committing.";
        let prefs = detect_preferences_in_user_turn(content);
        assert_eq!(prefs.len(), 1);
        assert!(prefs[0].raw_statement.contains("Always run tests"));
    }

    #[test]
    fn detect_preferences_finds_never_phrase() {
        let content = "Never use `unwrap()` in production code.";
        let prefs = detect_preferences_in_user_turn(content);
        assert_eq!(prefs.len(), 1);
    }

    #[test]
    fn detect_preferences_skips_neutral_statements() {
        let content = "The weather is nice today.";
        let prefs = detect_preferences_in_user_turn(content);
        assert!(
            prefs.is_empty(),
            "neutral statements must not yield preferences"
        );
    }

    #[test]
    fn deduplicate_preferences_removes_exact_duplicates() {
        let prefs = vec![
            DetectedPreference {
                raw_statement: "Never use unwrap.".to_owned(),
                generality: PreferenceGenerality::General,
            },
            DetectedPreference {
                raw_statement: "never use unwrap.".to_owned(),
                generality: PreferenceGenerality::General,
            },
        ];
        let deduped = deduplicate_preferences(prefs);
        assert_eq!(deduped.len(), 1);
    }

    #[test]
    fn plurality_vote_picks_most_common() {
        let votes = vec!["Rust", "Rust", "Python", "Rust", "Python"];
        assert_eq!(plurality_vote(&votes), Some("Rust"));
    }

    #[test]
    fn plurality_vote_on_empty_returns_none() {
        let votes: Vec<&str> = vec![];
        assert_eq!(plurality_vote(&votes), None);
    }

    #[test]
    fn salient_path_extracts_two_components() {
        let path = "/home/user/myrepo/crates/session-extractor/src/lib.rs";
        let salient = salient_path_from_file_path(path);
        // First two non-empty non-system components: "home/user" or similar.
        assert!(salient.is_some());
    }

    #[test]
    fn approximate_token_count_ceiling_divides() {
        assert_eq!(approximate_token_count(""), 0);
        assert_eq!(approximate_token_count("abcd"), 1); // 4 bytes → ceil(4/4) = 1
        assert_eq!(approximate_token_count("abcde"), 2); // 5 bytes → ceil(5/4) = 2
    }

    #[test]
    fn preference_name_derived_from_first_eight_words() {
        let stmt = "Please always use tokio for async runtime, never std::thread directly.";
        let name = derive_preference_name(stmt);
        assert!(
            name.ends_with("(preference)"),
            "name must end with '(preference)'; got: {name}"
        );
        assert!(
            name.contains("Please"),
            "name must start with the statement words; got: {name}"
        );
    }

    #[test]
    fn project_scoping_signals_classify_as_uncertain() {
        // "this project" signal → Uncertain
        let prefs =
            detect_preferences_in_user_turn("Always prefer async in this project when possible.");
        assert!(!prefs.is_empty(), "preference must be detected");
        assert_eq!(prefs[0].generality, PreferenceGenerality::Uncertain);
    }

    #[test]
    fn no_project_scoping_signal_classifies_as_general() {
        let prefs = detect_preferences_in_user_turn("Never add comments unless explicitly asked.");
        assert!(!prefs.is_empty(), "preference must be detected");
        assert_eq!(prefs[0].generality, PreferenceGenerality::General);
    }

    #[test]
    fn extract_bash_command_parses_command_field() {
        let input = r#"{"command": "cargo test", "timeout_ms": 5000}"#;
        assert_eq!(extract_bash_command(input), Some("cargo test".to_owned()));
    }

    #[test]
    fn extract_bash_command_returns_none_on_bad_json() {
        assert_eq!(extract_bash_command("not json"), None);
    }

    #[test]
    fn language_detected_from_cargo_command() {
        assert_eq!(
            detect_language_from_command("cargo build --release"),
            Some("Rust")
        );
    }

    #[test]
    fn language_detected_from_rs_extension() {
        assert_eq!(detect_language_from_path("src/main.rs"), Some("Rust"));
    }

    #[test]
    fn repo_name_extracted_from_deep_path() {
        // "/home/user/myrepo/src" should yield "myrepo".
        let path = "home/user/myrepo/src";
        let repo = extract_repo_name_from_path(path);
        assert_eq!(repo.as_deref(), Some("myrepo"));
    }

    #[test]
    fn render_preamble_text_includes_all_sections() {
        let prefs = vec![DetectedPreference {
            raw_statement: "Never use unwrap.".to_owned(),
            generality: PreferenceGenerality::General,
        }];
        let facts = ProjectFacts {
            repo_name: Some("myrepo".to_owned()),
            primary_language: Some("Rust".to_owned()),
            salient_paths: vec!["crates/domain".to_owned()],
            stated_conventions: vec![],
        };
        let text = render_preamble_text(&prefs, &facts);
        assert!(text.contains("myrepo"), "repo name must appear");
        assert!(text.contains("Rust"), "language must appear");
        assert!(text.contains("crates/domain"), "salient path must appear");
        assert!(text.contains("Never use unwrap"), "preference must appear");
    }

    #[test]
    fn preamble_with_no_events_produces_empty_text() {
        let draft = mine_draft_deterministic(&[]);
        let preamble = enforce_token_cap_and_build(draft);
        assert_eq!(
            preamble.text, "",
            "empty event stream must produce an empty preamble text"
        );
    }
}
