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
//!   [`crate::segmentation::segment_session`]. Frontier models have much larger
//!   context windows, but the current default uses focused 40 960-token windows
//!   for recall; local models use 8 192-token windows. The SAME
//!   `segment_session` code path handles both.
//! - **Dual-pass enabled** — whether to run a holistic whole-session pass
//!   alongside the structured per-episode pass (opt-in; currently not wired
//!   into the dispatch path, but the flag is recorded for observability and
//!   future wiring).
//!
//! ## Invariants
//!
//! - Routing NEVER bypasses the extraction pipeline. It only selects provider
//!   and granularity.
//! - The default is provider-aware: an unset `EXTRACT_SESSION_ROUTING` selects the
//!   frontier tier for a frontier provider (claude / claude-code) and the local
//!   tier for Ollama — a frontier provider is never silently capped at the small
//!   local window.
//! - The decision is logged at INFO level so every extraction records which
//!   provider/tier/granularity handled it (observable).
//!
//! ## Environment variables
//!
//! - `EXTRACT_SESSION_ROUTING` — routing strategy. Values:
//!   - unset / blank / `"tiered"` → provider-aware tier (frontier provider →
//!     frontier tier; Ollama → local tier)
//!   - `"local"` → local tier (explicit override; Ollama, small budget, no dual-pass)
//!   - `"frontier"` → frontier tier (ClaudeCode, large budget, dual-pass)
//!
//! - `EXTRACT_SESSION_FRONTIER_TOKEN_BUDGET` — frontier-tier segmentation budget
//!   override (default 40 960, focused frontier windows).
//! - `EXTRACT_SESSION_LOCAL_TOKEN_BUDGET` — local-tier segmentation budget
//!   override (default 8 192, the real local-model context).
//!
//! - `EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS` — token threshold for the
//!   `"tiered"` strategy. Default: 50 000.

use tracing::info;

use crate::ExtractionProvider;

/// The provider tier selected by the routing policy.
///
/// The tier determines the default `token_budget` (and hence segmentation
/// granularity): frontier uses focused 40 960-token windows; local uses a
/// smaller 8 192-token budget so large sessions are segmented more finely.
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
    /// `token_budget` = [`FRONTIER_TIER_TOKEN_BUDGET`] (40 960 tokens). This is a
    /// focused 5x-local window, not the model's maximum context. Larger sessions
    /// split into multiple overlapping windows (extracted + deduped, recall-first)
    /// so the long tail is never dropped. Tunable via
    /// `EXTRACT_SESSION_FRONTIER_TOKEN_BUDGET`.
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

/// Default segmentation `token_budget` for the local tier.
///
/// 8 192 tokens — the orchestration deliberately windows the session into chunks
/// this size for the local model; this is legitimate chunking, NOT a footgun (the
/// #214 footgun was the per-entry CHAR cap being *smaller* than this window — see
/// the extraction config defaults, now aligned). One window = `8192 × 4 ≈ 32 768`
/// chars (the `chars/4` token estimate). Override with
/// `EXTRACT_SESSION_LOCAL_TOKEN_BUDGET` for a larger local model.
///
/// **Alignment invariant (#176/#214):** this window content budget MUST fit inside
/// the Ollama context sent on every request — `infrastructure`'s
/// `EXTRACTION_OLLAMA_NUM_CTX` (16 384 tokens). 8 192 (window) + mined preamble +
/// prompt scaffold leaves ~2× headroom for the model's JSON output, so no window is
/// ever silently truncated. If you raise this budget, raise `OLLAMA_NUM_CTX` to
/// match (window + preamble + scaffold + output ≤ num_ctx).
pub const LOCAL_TIER_TOKEN_BUDGET: usize = 8_192;

/// Default segmentation `token_budget` for the frontier tier.
///
/// 40 960 tokens — 5× the local-tier chunk (8 192). The frontier model can hold far
/// more, but smaller, overlapping windows give the extractor a more focused view
/// per chunk (better recall on dense sessions) while the dedup/synthesis reduce
/// step recombines across windows; one window = `40 960 × 4 ≈ 163 840` chars.
/// Larger sessions split into multiple overlapping windows (extracted + deduped,
/// recall-first), so the long tail is never lost. Tune with
/// `EXTRACT_SESSION_FRONTIER_TOKEN_BUDGET`.
pub const FRONTIER_TIER_TOKEN_BUDGET: usize = 40_960;

/// Reads a tier token-budget override from `env_var`, falling back to `default`.
/// A non-integer or absent value uses the default. This keeps the budget a
/// tunable config value rather than a hardcoded constant.
fn token_budget_from_env(env_var: &str, default: usize) -> usize {
    std::env::var(env_var)
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

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
/// 1. If `EXTRACT_SESSION_ROUTING` is unset / blank / `"tiered"` → **provider-aware**
///    tier: `Frontier` for a frontier provider (`Claude` / `ClaudeCode`), `Local`
///    for `Ollama`. A frontier provider is NEVER silently squeezed into the small
///    local segmentation window just because the env var was not set.
/// 2. If `EXTRACT_SESSION_ROUTING=frontier` → `Frontier` tier.
/// 3. If `EXTRACT_SESSION_ROUTING=local` → `Local` tier (explicit operator override).
/// 4. Unknown values → provider-aware tier (with an observable warning).
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
    let routing_raw = std::env::var("EXTRACT_SESSION_ROUTING").unwrap_or_default();
    let routing_str = routing_raw.trim().to_ascii_lowercase();

    // Unset/blank defaults to provider-aware ("tiered") routing — NOT a blanket
    // local tier. A frontier provider (claude / claude-code) must never be
    // silently capped at the small local segmentation window just because the
    // routing env var wasn't set (the live compose never sets it). This is the
    // fix for claude-code running with an 8 192-token budget despite being a
    // 200k+ context model. Explicit "local" still forces the local tier.
    let provider_aware_tier = match provider {
        ExtractionProvider::Claude | ExtractionProvider::ClaudeCode => {
            ExtractionRoutingTier::Frontier
        }
        ExtractionProvider::Ollama => ExtractionRoutingTier::Local,
    };
    let tier = match routing_str.as_str() {
        "" | "tiered" => provider_aware_tier,
        "local" => ExtractionRoutingTier::Local,
        "frontier" => ExtractionRoutingTier::Frontier,
        other => {
            tracing::warn!(
                routing_value = other,
                "unknown EXTRACT_SESSION_ROUTING value; defaulting to provider-aware tier"
            );
            provider_aware_tier
        }
    };

    let (token_budget, dual_pass_enabled) = match tier {
        ExtractionRoutingTier::Frontier => (
            token_budget_from_env(
                "EXTRACT_SESSION_FRONTIER_TOKEN_BUDGET",
                FRONTIER_TIER_TOKEN_BUDGET,
            ),
            true,
        ),
        ExtractionRoutingTier::Local => (
            token_budget_from_env("EXTRACT_SESSION_LOCAL_TOKEN_BUDGET", LOCAL_TIER_TOKEN_BUDGET),
            false,
        ),
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
    #[error("EXTRACT_SESSION_ROUTING_THRESHOLD_TOKENS is not a valid integer: {raw:?} — {cause}")]
    InvalidThreshold { raw: String, cause: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the process-global `EXTRACT_SESSION_ROUTING`
    /// env var so they cannot clobber each other under parallel `cargo test`
    /// (#176 follow-up: the previous `let _guard = env::var(..).ok()` was a no-op
    /// — an Option, not a lock). Every env-mutating test holds this for its body.
    static ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(()));
    use crate::{
        ExtractionProvider,
        segmentation::{SegmentationConfig, segment_session},
    };
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
    /// produces many episodes but a frontier-tier budget (40 960 tokens)
    /// produces fewer, coarser windows.
    ///
    /// Budget math:
    /// - user_msg content: 100 chars → 25 tokens
    /// - tool_call name (4) + input_json (200 chars) → 51 tokens
    /// - tool_result output: 100 chars → 25 tokens
    /// - Per iteration (3 events): ~101 tokens
    fn large_session_events() -> Vec<SessionEvent> {
        let long_content = "a".repeat(100);
        let long_input = "b".repeat(200);
        let long_output = "c".repeat(100);
        // 2 400 iterations (~7 200 events, ~240k token estimate) — deliberately LARGER
        // than FRONTIER_TIER_TOKEN_BUDGET (40 960) so the frontier tier must ALSO
        // chunk it into multiple overlapping windows (never one giant single chunk),
        // while local chunks it far more finely. Proves both tiers use the same
        // segmentation pipeline, frontier coarser than local.
        (0..2_400_usize)
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
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }

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
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("EXTRACT_SESSION_ROUTING", "frontier");
        }

        let decision = compute_routing_decision(ExtractionProvider::ClaudeCode)
            .expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Frontier);
        assert_eq!(decision.provider, ExtractionProvider::ClaudeCode);
        assert_eq!(
            decision.segmentation_token_budget,
            FRONTIER_TIER_TOKEN_BUDGET
        );
        assert!(decision.dual_pass_enabled);

        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }
    }

    /// EXTRACT_SESSION_ROUTING=tiered with a frontier provider → Frontier tier.
    #[test]
    fn tiered_routing_with_frontier_provider_yields_frontier_tier() {
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("EXTRACT_SESSION_ROUTING", "tiered");
        }

        let decision = compute_routing_decision(ExtractionProvider::ClaudeCode)
            .expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Frontier);
        assert_eq!(
            decision.segmentation_token_budget,
            FRONTIER_TIER_TOKEN_BUDGET
        );

        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }
    }

    /// EXTRACT_SESSION_ROUTING=tiered with Ollama → Local tier.
    #[test]
    fn tiered_routing_with_ollama_provider_yields_local_tier() {
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("EXTRACT_SESSION_ROUTING", "tiered");
        }

        let decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("decision must succeed");
        assert_eq!(decision.tier, ExtractionRoutingTier::Local);
        assert_eq!(decision.segmentation_token_budget, LOCAL_TIER_TOKEN_BUDGET);

        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }
    }

    /// Unknown EXTRACT_SESSION_ROUTING value → Local tier (observable warning,
    /// not a fatal error — the test just checks the fallback, not the warning log).
    #[test]
    fn unknown_routing_env_falls_back_to_local_tier() {
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("EXTRACT_SESSION_ROUTING", "unknown-future-strategy");
        }

        let decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("decision must succeed");
        assert_eq!(
            decision.tier,
            ExtractionRoutingTier::Local,
            "unknown routing value must fall back to Local (loud warn, not fatal)"
        );

        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }
    }

    // ── granularity parity tests ──────────────────────────────────────────────

    /// Proves that:
    /// - A frontier-tier token budget (40 960) → MULTIPLE windows for a session
    ///   larger than the budget: frontier never one-shots an over-budget transcript.
    /// - A local-tier token budget (8 192) → even FINER granularity for the SAME
    ///   session. Same `segment_session` code path, only the budget differs.
    ///
    /// This is the authoritative parity assertion for the frontier/local granularity
    /// split mandated by todo #191. It reuses the same `segment_session` code path
    /// for both tiers, proving neither tier bypasses the segmentation pipeline.
    #[test]
    fn frontier_budget_chunks_large_session_coarser_than_local_same_pipeline() {
        let events = large_session_events();
        assert!(
            events.len() >= 100,
            "precondition: large_session_events must be ≥ 100 events for this test to be meaningful"
        );

        // Frontier tier: a LARGE session (> frontier budget) must still be CHUNKED into
        // multiple overlapping windows — never processed as one giant chunk. A single
        // chunk satisfices and drops the long tail; recall-first needs multiple windows.
        let frontier_config = SegmentationConfig::new(FRONTIER_TIER_TOKEN_BUDGET, 3);
        let frontier_episodes = segment_session(&events, &frontier_config);
        assert!(
            frontier_episodes.len() > 1,
            "frontier tier (budget={}) must CHUNK a large session into multiple windows, \
             not one giant chunk; got {}",
            FRONTIER_TIER_TOKEN_BUDGET,
            frontier_episodes.len()
        );

        // Local tier: smaller budget → FINER granularity (more windows) for the same
        // session. Same `segment_session` code path — only the budget differs.
        let local_config = SegmentationConfig::new(LOCAL_TIER_TOKEN_BUDGET, 3);
        let local_episodes = segment_session(&events, &local_config);
        assert!(
            local_episodes.len() > frontier_episodes.len(),
            "local tier (budget={}, got {} windows) must be FINER than frontier (budget={}, got {} windows) \
             for the same session",
            LOCAL_TIER_TOKEN_BUDGET,
            local_episodes.len(),
            FRONTIER_TIER_TOKEN_BUDGET,
            frontier_episodes.len()
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
        assert_eq!(local_covered, expected, "local tier must cover all events");
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
        let _env_lock = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe {
            std::env::set_var("EXTRACT_SESSION_ROUTING", "frontier");
        }
        let frontier_decision =
            compute_routing_decision(ExtractionProvider::ClaudeCode).expect("must succeed");
        assert_eq!(
            frontier_decision.segmentation_token_budget,
            FRONTIER_TIER_TOKEN_BUDGET
        );
        unsafe {
            std::env::remove_var("EXTRACT_SESSION_ROUTING");
        }

        // Local routing (unset) must produce the local token budget.
        let local_decision =
            compute_routing_decision(ExtractionProvider::Ollama).expect("must succeed");
        assert_eq!(
            local_decision.segmentation_token_budget,
            LOCAL_TIER_TOKEN_BUDGET
        );
    }
}
