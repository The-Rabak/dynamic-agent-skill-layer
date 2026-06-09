use std::path::Path;

use domain::SubunitType;
use serde::Deserialize;

use crate::extraction::ExtractedSubunit;

/// Output of the deterministic markdown extraction pass for a single SKILL.md.
///
/// All multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) are advisory source data for T04 and
/// T05 downstream.  They are WRITE-AHEAD: populated from frontmatter now,
/// consumed by embedding-view construction (T04) and typed-edge proposals (T05)
/// later.  They never influence subunit content or the ℓ₁ embedding text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralExtraction {
    pub skill_name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subunits: Vec<ExtractedSubunit>,
    /// Short task triggers where this skill applies. Empty when absent from frontmatter.
    pub use_when: Vec<String>,
    /// Situations where this skill should NOT be applied. Empty when absent.
    pub avoid_when: Vec<String>,
    /// File types, protocols, config names the skill applies to. Empty when absent.
    pub artifacts: Vec<String>,
    /// Commands, libraries, frameworks, APIs used. Empty when absent.
    pub tools: Vec<String>,
    /// Verifier-critical constraints. Empty when absent.
    pub invariants: Vec<String>,
    /// Prerequisites assumed by this skill. Empty when absent.
    pub requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill. Empty when absent.
    pub produces: Vec<String>,
}

/// Authoritative metadata carried in a SKILL.md YAML frontmatter block.
///
/// This is the **single source of truth** for `name`, `description`, and `tags`
/// in the unified SKILL.md format. Every field is optional so the reader can
/// gracefully parse partial frontmatter (and so files written by older or
/// hand-authored sources still load). When a field is present and non-empty it
/// overrides anything inferred from the markdown body.
///
/// `suggested_tags` is accepted as a backward-compatibility alias for `tags`:
/// pending drafts emitted before the format was unified used that key, and we
/// must keep reading them correctly without re-running extraction.
///
/// The multi-view fields (`use_when`, `avoid_when`, `artifacts`, `tools`,
/// `invariants`, `requires`, `produces`) are OPTIONAL advisory source data
/// for T04 (multi-view dense/BM25 matching) and T05 (typed-edge proposals).
/// They are WRITE-AHEAD: populated here at parse time, consumed downstream.
/// Absent fields deserialize to empty `Vec` — never a parse failure.
#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    suggested_tags: Option<Vec<String>>,
    /// Task triggers where this skill applies (multi-view source for T04).
    #[serde(default)]
    use_when: Vec<String>,
    /// Situations where this skill should NOT be applied (multi-view source for T04).
    #[serde(default)]
    avoid_when: Vec<String>,
    /// File types, protocols, config names the skill applies to (T05 edge source).
    #[serde(default)]
    artifacts: Vec<String>,
    /// Commands, libraries, frameworks, services, models, or APIs (T05 edge source).
    #[serde(default)]
    tools: Vec<String>,
    /// Verifier-critical constraints (T04/T05 source).
    #[serde(default)]
    invariants: Vec<String>,
    /// Prerequisites assumed by this skill (T05 typed-edge source).
    #[serde(default)]
    requires: Vec<String>,
    /// Outcomes or artifacts produced by following this skill (T05 edge source).
    #[serde(default)]
    produces: Vec<String>,
}

impl SkillFrontmatter {
    /// Canonical tags: prefer the `tags` key, fall back to the legacy
    /// `suggested_tags` alias. Entries are trimmed and empties dropped.
    fn resolved_tags(&self) -> Vec<String> {
        self.tags
            .as_ref()
            .or(self.suggested_tags.as_ref())
            .map(|tags| {
                tags.iter()
                    .map(|tag| tag.trim().to_owned())
                    .filter(|tag| !tag.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Splits a SKILL.md document into its optional YAML frontmatter block and the
/// markdown body that follows it.
///
/// A document has frontmatter only when it *starts* with a `---\n` fence and a
/// closing `\n---\n` fence follows. In every other case the whole document is
/// the body — including a stray opening fence with no closing fence, so we never
/// silently swallow content. Returning the body separately is what prevents the
/// frontmatter (the `---` fence lines and any YAML list items like `- tag`) from
/// leaking into the description or the structural subunits.
fn split_frontmatter(markdown: &str) -> (Option<&str>, &str) {
    let Some(after_open) = markdown.strip_prefix("---\n") else {
        return (None, markdown);
    };
    match after_open.split_once("\n---\n") {
        Some((frontmatter, body)) => (Some(frontmatter), body),
        None => (None, markdown),
    }
}

/// Applies deterministic markdown rules to extract stable graph subunits.
///
/// The unified SKILL.md format is YAML frontmatter (authoritative metadata)
/// followed by a markdown body (human prose + `## Procedures/Conventions/...`
/// sections that become subunits). The parse order is:
///
/// 1. Split off the frontmatter; scan the **body only** for the `# title`, an
///    optional `tags:` line, the first prose paragraph (description), and the
///    `##` sections (subunits).
/// 2. Override `name`, `description`, and `tags` with the frontmatter values
///    whenever they are present and non-empty — the frontmatter wins.
///
/// Body-only files (no frontmatter) still parse via step 1 alone, so legacy and
/// hand-authored skills keep working.
pub fn extract_structural_subunits(path: &Path, markdown: &str) -> StructuralExtraction {
    let (frontmatter_src, body) = split_frontmatter(markdown);
    let frontmatter =
        frontmatter_src.and_then(|src| serde_yaml::from_str::<SkillFrontmatter>(src).ok());

    let mut skill_name = path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("unnamed-skill")
        .to_owned();
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut subunits = Vec::new();
    let mut current_kind = SubunitType::Summary;

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            if !title.trim().is_empty() {
                skill_name = title.trim().to_owned();
            }
            continue;
        }

        if let Some(tag_line) = trimmed.strip_prefix("tags:") {
            tags.extend(
                tag_line
                    .split(',')
                    .map(str::trim)
                    .filter(|candidate| !candidate.is_empty())
                    .map(str::to_owned),
            );
            continue;
        }

        match trimmed.to_ascii_lowercase().as_str() {
            "## procedures" => current_kind = SubunitType::Procedure,
            "## conventions" => current_kind = SubunitType::Convention,
            "## assets" => current_kind = SubunitType::Asset,
            "## evidence" => current_kind = SubunitType::Evidence,
            "## summary" => current_kind = SubunitType::Summary,
            _ => {
                if description.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
                    description = trimmed.to_owned();
                }
                if let Some(content) = trimmed.strip_prefix("- ") {
                    subunits.push(ExtractedSubunit {
                        kind: current_kind,
                        title: format!("{current_kind:?} note"),
                        content: content.trim().to_owned(),
                    });
                }
            }
        }
    }

    // Frontmatter is authoritative: override body-inferred values when present.
    // Multi-view fields come exclusively from frontmatter — they have no body equivalent.
    let mut use_when = Vec::new();
    let mut avoid_when = Vec::new();
    let mut artifacts = Vec::new();
    let mut tools = Vec::new();
    let mut invariants = Vec::new();
    let mut requires = Vec::new();
    let mut produces = Vec::new();

    if let Some(frontmatter) = &frontmatter {
        if let Some(name) = frontmatter
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            skill_name = name.to_owned();
        }
        if let Some(frontmatter_description) = frontmatter
            .description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty())
        {
            description = frontmatter_description.to_owned();
        }
        let frontmatter_tags = frontmatter.resolved_tags();
        if !frontmatter_tags.is_empty() {
            tags = frontmatter_tags;
        }
        // Multi-view fields are read directly from frontmatter (no body fallback).
        use_when = frontmatter.use_when.clone();
        avoid_when = frontmatter.avoid_when.clone();
        artifacts = frontmatter.artifacts.clone();
        tools = frontmatter.tools.clone();
        invariants = frontmatter.invariants.clone();
        requires = frontmatter.requires.clone();
        produces = frontmatter.produces.clone();
    }

    if description.is_empty() {
        description = "No description provided".to_owned();
    }

    StructuralExtraction {
        skill_name,
        description,
        tags,
        subunits,
        use_when,
        avoid_when,
        artifacts,
        tools,
        invariants,
        requires,
        produces,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// The exact shape the extraction writer emits: YAML frontmatter (with the
    /// authoritative `description` + `tags`) followed by a `# title`, prose, and
    /// `##` sections. Regression for #224: the reader must read the frontmatter
    /// `description` — NOT capture the opening `---` fence as the description.
    #[test]
    fn frontmatter_description_is_authoritative_and_fence_does_not_leak() {
        let markdown = "---\n\
name: http-router-security-defaults\n\
description: Apply mandatory security defaults to every HTTP router.\n\
tags:\n\
- security\n\
- http\n\
generality: general\n\
---\n\
\n\
# http-router-security-defaults\n\
\n\
Apply mandatory security defaults to every HTTP router.\n\
\n\
## Procedures\n\
- Set secure headers on every response\n\
- Reject requests without a CSRF token\n";

        let extraction = extract_structural_subunits(
            Path::new("http-router-security-defaults/SKILL.md"),
            markdown,
        );

        assert_eq!(
            extraction.description, "Apply mandatory security defaults to every HTTP router.",
            "description must come from frontmatter, never the `---` fence"
        );
        assert_ne!(extraction.description, "---", "the #224 bug must not recur");
        assert_eq!(extraction.skill_name, "http-router-security-defaults");
        assert_eq!(extraction.tags, vec!["security", "http"]);
        // Only the two body procedures are subunits — the frontmatter YAML list
        // items (`- security`, `- http`) must NOT leak in as subunits.
        assert_eq!(
            extraction.subunits.len(),
            2,
            "frontmatter list items must not leak into subunits; got {:?}",
            extraction.subunits
        );
        assert!(
            extraction
                .subunits
                .iter()
                .all(|subunit| subunit.kind == SubunitType::Procedure),
            "both subunits should be procedures from the body"
        );
    }

    /// Backward-compat: the legacy `suggested_tags` frontmatter key (emitted by
    /// pending drafts before the format was unified) is still read as tags.
    #[test]
    fn legacy_suggested_tags_alias_is_read_as_tags() {
        let markdown = "---\n\
name: rust-testing\n\
description: Rust testing patterns.\n\
suggested_tags:\n\
- rust\n\
- testing\n\
---\n\
\n\
# rust-testing\n\
\n\
Rust testing patterns.\n";

        let extraction = extract_structural_subunits(Path::new("rust-testing/SKILL.md"), markdown);

        assert_eq!(extraction.tags, vec!["rust", "testing"]);
        assert_eq!(extraction.description, "Rust testing patterns.");
    }

    /// Body-only files (no frontmatter) must still parse via the body heuristic
    /// so hand-authored / legacy skills keep working.
    #[test]
    fn body_only_skill_without_frontmatter_still_parses() {
        let markdown = "# rust-file-io\n\
tags: rust, file, io\n\
\n\
Async and sync file I/O patterns for Rust.\n\
\n\
## Procedures\n\
- Use tokio::fs::read_to_string for small files\n\
\n\
## Conventions\n\
- Validate paths against an allowed root\n";

        let extraction = extract_structural_subunits(Path::new("rust-file-io/SKILL.md"), markdown);

        assert_eq!(extraction.skill_name, "rust-file-io");
        assert_eq!(
            extraction.description,
            "Async and sync file I/O patterns for Rust."
        );
        assert_eq!(extraction.tags, vec!["rust", "file", "io"]);
        assert_eq!(extraction.subunits.len(), 2);
    }

    /// A genuinely empty/contentless skill still yields the explicit
    /// "No description provided" sentinel rather than a fence or YAML key.
    #[test]
    fn missing_description_falls_back_to_explicit_sentinel() {
        let markdown = "---\nname: empty-skill\n---\n\n# empty-skill\n";
        let extraction = extract_structural_subunits(Path::new("empty-skill/SKILL.md"), markdown);
        assert_eq!(extraction.description, "No description provided");
        assert_eq!(extraction.skill_name, "empty-skill");
    }

    /// Multi-view optional fields round-trip through the frontmatter reader.
    ///
    /// Proves that `use_when`, `avoid_when`, `artifacts`, `tools`, `invariants`,
    /// `requires`, and `produces` — when present in the YAML frontmatter — are
    /// correctly parsed into `StructuralExtraction` and do NOT leak into subunits.
    #[test]
    fn multi_view_optional_fields_parse_from_frontmatter() {
        let markdown = "---\n\
name: docker-compose-local-dev\n\
description: Manage local dev services with docker compose.\n\
tags:\n\
- docker\n\
- dev\n\
use_when:\n\
- Starting local dev environment\n\
- Spinning up dependencies for integration tests\n\
avoid_when:\n\
- Deploying to production\n\
artifacts:\n\
- docker-compose.yml\n\
- .env.local\n\
tools:\n\
- docker\n\
- docker compose\n\
invariants:\n\
- All services must pass healthchecks before the stack is considered ready\n\
requires:\n\
- Docker Desktop >= 4.x installed\n\
produces:\n\
- Running local service stack accessible on localhost ports\n\
---\n\
\n\
# docker-compose-local-dev\n\
\n\
Manage local dev services with docker compose.\n\
\n\
## Procedures\n\
- Run docker compose up -d to start all services\n\
- Run docker compose down to stop them\n";

        let extraction =
            extract_structural_subunits(Path::new("docker-compose-local-dev/SKILL.md"), markdown);

        assert_eq!(extraction.skill_name, "docker-compose-local-dev");
        assert_eq!(
            extraction.description,
            "Manage local dev services with docker compose."
        );
        assert_eq!(extraction.tags, vec!["docker", "dev"]);

        assert_eq!(
            extraction.use_when,
            vec![
                "Starting local dev environment",
                "Spinning up dependencies for integration tests"
            ],
            "use_when must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.avoid_when,
            vec!["Deploying to production"],
            "avoid_when must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.artifacts,
            vec!["docker-compose.yml", ".env.local"],
            "artifacts must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.tools,
            vec!["docker", "docker compose"],
            "tools must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.invariants,
            vec!["All services must pass healthchecks before the stack is considered ready"],
            "invariants must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.requires,
            vec!["Docker Desktop >= 4.x installed"],
            "requires must round-trip from frontmatter"
        );
        assert_eq!(
            extraction.produces,
            vec!["Running local service stack accessible on localhost ports"],
            "produces must round-trip from frontmatter"
        );

        // Multi-view YAML list items must NOT become subunits.
        let body_subunit_count = extraction.subunits.len();
        assert_eq!(
            body_subunit_count, 2,
            "only the two body ## Procedures bullets should be subunits; \
             frontmatter multi-view items must not leak in as subunits, got {:?}",
            extraction.subunits
        );
    }

    /// Skills without any multi-view fields parse correctly with empty Vecs —
    /// absent optional fields must never cause a deserialize failure.
    #[test]
    fn absent_multi_view_fields_default_to_empty_vecs() {
        let markdown = "---\n\
name: rust-testing\n\
description: Run tests with cargo.\n\
tags:\n\
- rust\n\
---\n\
\n\
# rust-testing\n\
\n\
Run tests with cargo.\n\
\n\
## Procedures\n\
- Run cargo test --workspace\n\
- Check output for FAILED lines\n";

        let extraction = extract_structural_subunits(Path::new("rust-testing/SKILL.md"), markdown);

        assert_eq!(extraction.skill_name, "rust-testing");
        assert!(
            extraction.use_when.is_empty(),
            "absent use_when must default to empty Vec"
        );
        assert!(
            extraction.avoid_when.is_empty(),
            "absent avoid_when must default to empty Vec"
        );
        assert!(
            extraction.artifacts.is_empty(),
            "absent artifacts must default to empty Vec"
        );
        assert!(
            extraction.tools.is_empty(),
            "absent tools must default to empty Vec"
        );
        assert!(
            extraction.invariants.is_empty(),
            "absent invariants must default to empty Vec"
        );
        assert!(
            extraction.requires.is_empty(),
            "absent requires must default to empty Vec"
        );
        assert!(
            extraction.produces.is_empty(),
            "absent produces must default to empty Vec"
        );
    }
}
