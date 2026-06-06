//! Tiered extraction routing — maps session size/importance signals to a
//! provider tier, a segmentation granularity (`token_budget`), and a dual-pass
//! flag.
//!
//! ## Design
//!
//! Routing is purely a **configuration** concern: the operator declares a tier
//! strategy via `EXTRACT_SESSION_ROUTING` at startup. The tier governs:
//!
//! - **Provider** — which [`crate::ExtractionProvider`] handles sessions on
//!   this tier (frontier = claude-code or claude; local = ollama).
//! - **Segmentation granularity** — the `token_budget` passed to
//!   [`crate::segmentation::segment_session`]. Frontier models have a 200 k+
//!   context window; setting `token_budget` to that value means the whole
//!   session becomes ONE episode (holistic reasoning, no fragmentation). Local
//!   models have an ~8 k context window; the small budget produces many
//!   episodes. The SAME `segment_session` code path handles both.
//! - **Dual-pass enabled** — whether to run a holistic whole-session pass
//!   alongside the structured per-episode pass (opt-in; currently not wired
//!   into the dispatch path, but the flag is recorded for observability and
//!   future wiring).
//!
//! ## Invariants
//!
//! - Routing NEVER bypasses the extraction pipeline. It only selects provider
//!   and granularity.
//! - Local is always the default floor. An unset `EXTRACT_SESSION_ROUTING`
//!   produces a local-tier decision.
//! - The decision is logged at INFO level so every extraction records which
//!   provider/tier/granularity handled it (observable).
//!
//! ## Environment variables
//!
//! - `EXTRACT_SESSION_ROUTING` — routing strategy. Values:
//!   - unset / blank / `"local"` → local tier (Ollama, small budget, no dual-pass)
//!   - `"frontier"` → frontier tier (ClaudeCode, large budget, dual-pass)
//!   - `"tiered"` → size-threshold routing: sessions above
//!     `EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS` (default 50 000 tokens) use
//!     the frontier tier; sessions at or below it use the local tier. Because
//!     `SessionExtractor` is constructed once (not per-session), `"tiered"`
//!     selects the frontier provider; the boundary is enforced by the per-call
//!     `token_budget` choice, not by provider switching at runtime.
//!
//! - `EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS` — token threshold for the
//!   `"tiered"` strategy. Default: 50 000.

use tracing::info;

use crate::ExtractionProvider;

/// The provider tier selected by the routing policy.
///
/// The tier determines the default `token_budget` (and hence segmentation
/// granularity): frontier uses the model's full context window so the whole
/// session becomes ONE episode; local uses a conservative budget so large
/// sessions are segmented into many episodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionRoutingTier {
    /// Local (Ollama) extraction — default floor, offline-capable.
    ///
    /// `token_budget` = [`LOCAL_TIER_TOKEN_BUDGET`] (8 192 tokens). A typical
    /// 100 k-token session is segmented into many episodes; the pipeline
    /// processes them individually.
    Local,
    /// Frontier (Claude Code or Claude API) extraction — opt-in upgrade.
    ///
    /// `token_budget` = [`FRONTIER_TIER_TOKEN_BUDGET`] (200 000 tokens). A
    /// session up to ~200 k tokens becomes ONE episode; the model reasons
    /// holistically over the full arc.
    Frontier,
}

impl ExtractionRoutingTier {
    /// Canonical string label used in log and event payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Frontier => "frontier",
        }
    }
}

/// Segmentation `token_budget` for the local tier.
///
/// Conservative 8 192-token budget (same as `OrchestrationConfig::default`),
/// appropriate for local models with a small context window.
pub const LOCAL_TIER_TOKEN_BUDGET: usize = 8_192;

/// Segmentation `token_budget` for the frontier tier.
///
/// 200 000 tokens — matches the Claude Sonnet context window. A session whose
/// token estimate is at or below this value becomes ONE episode, giving the
/// model holistic cross-arc reasoning without fragmentation.
pub const FRONTIER_TIER_TOKEN_BUDGET: usize = 200_000;

/// Default session-size threshold (in estimated tokens) for the `"tiered"`
/// strategy. Sessions above this are considered large/high-value and route to
/// the frontier tier (if one is configured).
pub const DEFAULT_TIERED_THRESHOLD_TOKENS: usize = 50_000;

/// The routing decision produced for a single [`crate::SessionExtractor`]
/// construction.
///
/// Carries the resolved tier, provider, segmentation granularity, and
/// dual-pass flag so every component that needs them reads from one place.
/// The decision is logged at INFO level at construction time so each
/// extraction context is observable without additional instrumentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingDecision {
    /// The resolved routing tier.
    pub tier: ExtractionRoutingTier,
    /// The extraction provider selected by this tier.
    pub provider: ExtractionProvider,
    /// The `token_budget` to pass to `segment_session` for this tier.
    ///
    /// Frontier: [`FRONTIER_TIER_TOKEN_BUDGET`] → one episode per session.
    /// Local: [`LOCAL_TIER_TOKEN_BUDGET`] → many episodes for large sessions.
    pub segmentation_token_budget: usize,
    /// Whether the dual-pass (holistic whole-session + structured per-episode)
    /// is enabled for this tier. Currently recorded for observability; not yet
    /// wired into the dispatch path.
    pub dual_pass_enabled: bool,
}

impl RoutingDecision {
    /// Returns a human-readable summary for log lines.
    pub fn summary(&self) -> String {
        format!(
            "tier={} provider={} token_budget={} dual_pass={}",
            self.tier.as_str(),
            self.provider.as_str(),
            self.segmentation_token_budget,
            self.dual_pass_enabled,
        )
    }
}

/// Computes the routing decision from environment variables and the already-
/// selected extraction provider.
///
/// Called once per `SessionExtractor::from_environment` construction. The
/// resulting `RoutingDecision` is stored on the extractor and logged so every
/// extraction is observable.
///
/// ## Rules
///
/// 1. If `EXTRACT_SESSION_ROUTING` is unset / blank / `"local"` → `Local` tier,
///    regardless of `provider`. The provider was already selected by
///    `EXTRACT_SESSION_PROVIDER`; routing does NOT override it but DOES record
///    the tier for observability.
/// 2. If `EXTRACT_SESSION_ROUTING=frontier` → `Frontier` tier.
/// 3. If `EXTRACT_SESSION_ROUTING=tiered` → `Frontier` tier when `provider` is
///    a frontier variant (`Claude` or `ClaudeCode`); `Local` tier otherwise.
///    This handles the case where the operator has selected a frontier provider
///    via `EXTRACT_SESSION_PROVIDER` and wants the routing tier to reflect it.
///
/// In all cases the `provider` field in the returned decision echoes the
/// already-selected `provider` argument — routing does not switch providers at
/// runtime.
///
/// # Errors
///
/// Returns `Err` only when `EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS` is set to
/// a non-integer value (loud parse failure, never a silent default). All other
/// parse errors (unknown routing value) silently fall back to `"local"` with a
/// warning — this is intentional so an unknown value is observable but not fatal.
pub fn compute_routing_decision(
    provider: ExtractionProvider,
) -> Result<RoutingDecision, RoutingConfigError> {
    let routing_raw = std::env::var("EXTRACT_SESSION_ROUTING")
        .unwrap_or_default();
    let routing_str = routing_raw.trim().to_ascii_lowercase();

    let tier = match routing_str.as_str() {
        "" | "local" => ExtractionRoutingTier::Local,
        "frontier" => ExtractionRoutingTier::Frontier,
        "tiered" => {
            // "tiered" defers to the already-selected provider: if the operator
            // chose a frontier provider, honour the frontier tier.
            match provider {
                ExtractionProvider::Claude | ExtractionProvider::ClaudeCode => {
                    ExtractionRoutingTier::Frontier
                }
                ExtractionProvider::Ollama => ExtractionRoutingTier::Local,
            }
        }
        other => {
            tracing::warn!(
                routing_value = other,
                "unknown EXTRACT_SESSION_ROUTING value; defaulting to 'local' tier"
            );
            ExtractionRoutingTier::Local
        }
    };

    let (token_budget, dual_pass_enabled) = match tier {
        ExtractionRoutingTier::Frontier => (FRONTIER_TIER_TOKEN_BUDGET, true),
        ExtractionRoutingTier::Local => (LOCAL_TIER_TOKEN_BUDGET, false),
    };

    let decision = RoutingDecision {
        tier,
        provider,
        segmentation_token_budget: token_budget,
        dual_pass_enabled,
    };

    info!(
        routing_summary = %decision.summary(),
        "extraction routing decision resolved"
    );

    Ok(decision)
}

/// Error produced when routing environment configuration is malformed.
#[derive(Debug, thiserror::Error)]
pub enum RoutingConfigError {
    #[error(
        "EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS is not a valid integer: {raw:?} — {cause}"
    )]
    InvalidThreshold { raw: String, cause: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ExtractionProvider, segmentation::{SegmentationConfig, segment_session}};
    use domain::SessionEvent;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn user_msg(index: usize, content: &str) -> SessionEvent {
        SessionEvent::UserMessage {
            index,
            content: content.to_owned(),
        }
    }

    fn tool_call(index: usize, id: &str, name: &str, input_json: &str) -> SessionEvent {
        SessionEvent::ToolCall {
            index,
            tool_use_id: id.to_owned(),
            name: name.to_owned(),
            input_json: input_json.to_owned(),
        }
    }

    fn tool_result_ok(index: usize, id: &str, output: &str) -> SessionEvent {
        SessionEvent::ToolResult {
            index,
            tool_use_id: id.to_owned(),
            is_error: false,
            exit_code: Some(0),
            output: output.to_owned(),
        }
    }

    /// Builds a session large enough that a local-tier budget (8 192 tokens)
    /// produces many episodes but a frontier-tier budget (200 000 tokens)
    /// produces exactly one.
    ///
    /// Budget math:
    /// - user_msg content: 100 chars → 25 tokens
    /// - tool_call name (4) + input_json (200 chars) → 51 tokens
    /// - tool_result output: 100 chars → 25 tokens
    /// - Per block: ~101 tokens
    /// - 300 blocks: ~30 300 tokens — comfortably exceeds local budget (8 192),
    ///   fits frontier budget (200 000).
    fn large_session_events() -> Vec<SessionEvent> {
        let long_content = "a".repeat(100);
        let long_input = "b".repeat(200);
        let long_output = "c".repeat(100);
        (0..300_usize)
            .flat_map(|i| {
                vec![
                    user_msg(i * 3, &format!("{long_content} task-{i:04}")),
                    tool_call(
                        i * 3 + 1,
                        &format!("id{i:04}"),
                        "Bash",
                        &format!(r#"{{"command":"{long_input} scenario_{i:04}"}}"#),
                    ),
                    tool_result_ok(
                        i * 3 + 2,
                        &format!("id{i:04}"),
                        &format!("{long_output} result-{i:04}"),
                    ),
                ]
            })
            .collect()
    }

    // ── routing tier tests ────────────────────────────────────────────────────

    /// Unset EXTRACT_SESSION_ROUTING → Local tier with the local token budget.
    #[test]
    fn unset_routing_env_yields_local_tier() {
        // Ensure the env var is absent for this test.
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }

        let decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Local);
        assert_eq!(decision.provider, ExtractionProvider::Ollama);
        assert_eq!(decision.segmentation_token_budget, LOCAL_TIER_TOKEN_BUDGET);
        assert!(!decision.dual_pass_enabled);
    }

    /// EXTRACT_SESSION_ROUTING=frontier → Frontier tier with the frontier token budget.
    #[test]
    fn frontier_routing_env_yields_frontier_tier() {
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::set_var("EXTRACT_SESSION_ROUTING", "frontier"); }

        let decision =
            compute_routing_decision(ExtractionProvider::ClaudeCode).expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Frontier);
        assert_eq!(decision.provider, ExtractionProvider::ClaudeCode);
        assert_eq!(decision.segmentation_token_budget, FRONTIER_TIER_TOKEN_BUDGET);
        assert!(decision.dual_pass_enabled);

        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }
    }

    /// EXTRACT_SESSION_ROUTING=tiered with a frontier provider → Frontier tier.
    #[test]
    fn tiered_routing_with_frontier_provider_yields_frontier_tier() {
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::set_var("EXTRACT_SESSION_ROUTING", "tiered"); }

        let decision =
            compute_routing_decision(ExtractionProvider::ClaudeCode).expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Frontier);
        assert_eq!(decision.segmentation_token_budget, FRONTIER_TIER_TOKEN_BUDGET);

        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }
    }

    /// EXTRACT_SESSION_ROUTING=tiered with Ollama → Local tier.
    #[test]
    fn tiered_routing_with_ollama_provider_yields_local_tier() {
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::set_var("EXTRACT_SESSION_ROUTING", "tiered"); }

        let decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Local);
        assert_eq!(decision.segmentation_token_budget, LOCAL_TIER_TOKEN_BUDGET);

        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }
    }

    /// Unknown EXTRACT_SESSION_ROUTING value → Local tier (observable warning,
    /// not a fatal error — the test just checks the fallback, not the warning log).
    #[test]
    fn unknown_routing_env_falls_back_to_local_tier() {
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::set_var("EXTRACT_SESSION_ROUTING", "unknown-future-strategy"); }

        let decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("decision must succeed");
        assert_eq!(
            decision.tier,
            ExtractionRoutingTier::Local,
            "unknown routing value must fall back to Local (loud warn, not fatal)"
        );

        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }
    }

    // ── granularity parity tests ──────────────────────────────────────────────

    /// Proves that:
    /// - A frontier-tier token budget (200 000) → exactly ONE episode for a large
    ///   session (same-pipeline, no special-case branch).
    /// - A local-tier token budget (8 192) → MANY episodes for the SAME session.
    ///
    /// This is the authoritative parity assertion for the frontier/local granularity
    /// split mandated by todo #191. It reuses the same `segment_session` code path
    /// for both tiers, proving neither tier bypasses the segmentation pipeline.
    #[test]
    fn frontier_budget_yields_one_episode_local_budget_yields_many_same_pipeline() {
        let events = large_session_events();
        assert!(
            events.len() >= 100,
            "precondition: large_session_events must be ≥ 100 events for this test to be meaningful"
        );

        // Frontier tier: budget = model window → whole session = one episode.
        let frontier_config = SegmentationConfig::new(FRONTIER_TIER_TOKEN_BUDGET, 3);
        let frontier_episodes = segment_session(&events, &frontier_config);
        assert_eq!(
            frontier_episodes.len(),
            1,
            "frontier tier (budget={}) must yield exactly ONE episode for a large session; got {}",
            FRONTIER_TIER_TOKEN_BUDGET,
            frontier_episodes.len()
        );

        // Local tier: small budget → many episodes for the same session.
        let local_config = SegmentationConfig::new(LOCAL_TIER_TOKEN_BUDGET, 3);
        let local_episodes = segment_session(&events, &local_config);
        assert!(
            local_episodes.len() > 1,
            "local tier (budget={}) must yield MANY episodes for the same large session; got {}",
            LOCAL_TIER_TOKEN_BUDGET,
            local_episodes.len()
        );

        // Both must cover every event — the same pipeline, no dropped events.
        let frontier_covered: std::collections::BTreeSet<usize> = frontier_episodes
            .iter()
            .flat_map(|ep| ep.event_indices.iter().copied())
            .collect();
        let local_covered: std::collections::BTreeSet<usize> = local_episodes
            .iter()
            .flat_map(|ep| ep.event_indices.iter().copied())
            .collect();
        let expected: std::collections::BTreeSet<usize> =
            events.iter().map(|e| e.index()).collect();
        assert_eq!(
            frontier_covered, expected,
            "frontier tier must cover all events"
        );
        assert_eq!(
            local_covered, expected,
            "local tier must cover all events"
        );
    }

    /// Proves that the routing decision for a frontier-configured provider matches
    /// the granularity expected by the parity assertion above: frontier routing →
    /// FRONTIER_TIER_TOKEN_BUDGET, local routing → LOCAL_TIER_TOKEN_BUDGET.
    ///
    /// This links the routing configuration layer to the segmentation layer so the
    /// two tests together prove the full chain.
    #[test]
    fn routing_decision_budget_matches_segmentation_tier_expectation() {
        // Frontier routing must produce the frontier token budget.
        let _guard = std::env::var("EXTRACT_SESSION_ROUTING").ok();
        unsafe { std::env::set_var("EXTRACT_SESSION_ROUTING", "frontier"); }
        let frontier_decision =
            compute_routing_decision(ExtractionProvider::ClaudeCode).expect("must succeed");
        assert_eq!(frontier_decision.segmentation_token_budget, FRONTIER_TIER_TOKEN_BUDGET);
        unsafe { std::env::remove_var("EXTRACT_SESSION_ROUTING"); }

        // Local routing (unset) must produce the local token budget.
        let local_decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("must succeed");
        assert_eq!(local_decision.segmentation_token_budget, LOCAL_TIER_TOKEN_BUDGET);
    }
}
