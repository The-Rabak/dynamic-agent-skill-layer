// Shared across e2e test binaries; #[path]-included per-binary, so any helper a given binary
// doesn't exercise is dead_code only as a per-binary compilation artifact, not a real orphan.
// Review for a genuine orphan before deleting any helper only one non-gate binary uses.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2EReport {
    pub test_name: String,
    pub test_id: String,
    pub started_at: String,
    pub duration_ms: u64,
    pub outcome: ReportOutcome,
    pub sections: Vec<ReportSection>,
    pub environment: EnvironmentSnapshot,
    pub contract_assertions: Vec<ContractAssertion>,
    pub degradation_events: Vec<DegradationEvent>,
    pub latency_samples: Vec<LatencySample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ReportOutcome {
    Passed,
    Failed { reason: String },
    Skipped { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSection {
    pub name: String,
    pub status: ReportOutcome,
    pub actions: Vec<ReportedAction>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportedAction {
    pub description: String,
    pub status: AssertionResult,
    pub side_effects: Vec<SideEffect>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result")]
pub enum AssertionResult {
    Passed,
    Failed { expected: String, actual: String },
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SideEffect {
    DbRowInserted { table: String },
    EventPublished { event_type: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub pg_version: String,
    pub qdrant_version: String,
    pub ollama_model: String,
    pub redis_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAssertion {
    pub contract_name: String,
    pub status: AssertionResult,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DegradationEvent {
    pub service: String,
    pub at: String,
    pub recovered: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencySample {
    pub stage: String,
    pub duration_ms: u64,
    pub at: String,
}

#[derive(Debug, Clone)]
pub struct ReportBuilder {
    test_name: String,
    started_at: String,
    sections: Vec<ReportSection>,
    contract_assertions: Vec<ContractAssertion>,
    degradation_events: Vec<DegradationEvent>,
    latency_samples: Vec<LatencySample>,
}

impl ReportBuilder {
    pub fn new(test_name: &str) -> Self {
        Self {
            test_name: test_name.to_owned(),
            started_at: chrono::Utc::now().to_rfc3339(),
            sections: Vec::new(),
            contract_assertions: Vec::new(),
            degradation_events: Vec::new(),
            latency_samples: Vec::new(),
        }
    }

    pub fn push_action(&mut self, section: &str, action: ReportedAction) {
        let section_millis = action.duration_ms;
        let action_outcome = match &action.status {
            AssertionResult::Passed => ReportOutcome::Passed,
            AssertionResult::Failed { expected, actual } => ReportOutcome::Failed {
                reason: format!("expected {expected}, got {actual}"),
            },
            AssertionResult::Skipped => ReportOutcome::Skipped {
                reason: "action was skipped".to_owned(),
            },
        };
        if let Some(existing) = self.sections.iter_mut().find(|s| s.name == section) {
            existing.duration_ms += section_millis;
            if matches!(action_outcome, ReportOutcome::Failed { .. })
                || (matches!(existing.status, ReportOutcome::Passed)
                    && matches!(action_outcome, ReportOutcome::Skipped { .. }))
            {
                existing.status = action_outcome;
            }
            existing.actions.push(action);
        } else {
            self.sections.push(ReportSection {
                name: section.to_owned(),
                status: action_outcome,
                actions: vec![action],
                duration_ms: section_millis,
            });
        }
    }

    pub fn record_degradation_event(&mut self, service: &str, recovered: bool, reason: &str) {
        self.degradation_events.push(DegradationEvent {
            service: service.to_owned(),
            at: chrono::Utc::now().to_rfc3339(),
            recovered,
            reason: reason.to_owned(),
        });
    }

    /// Push a raw `ContractAssertion` when the caller has already constructed it.
    /// Prefer `assert_contract` for inline pass/fail derivation.
    pub fn add_contract_assertion(&mut self, assertion: ContractAssertion) {
        self.contract_assertions.push(assertion);
    }

    /// Record a named contract assertion by evaluating `passed` at call time.
    ///
    /// Pushes a `ContractAssertion` whose `status` is `Passed` when `passed` is
    /// `true` and `Failed { expected, actual }` otherwise.  Returns `passed` so
    /// callers can chain: `assert!(builder.assert_contract(…));`.
    ///
    /// Prefer this over `add_contract_assertion` with a hardcoded `AssertionResult::Passed`.
    pub fn assert_contract(
        &mut self,
        name: &str,
        passed: bool,
        expected: &str,
        actual: &str,
        details: &str,
    ) -> bool {
        let status = if passed {
            AssertionResult::Passed
        } else {
            AssertionResult::Failed {
                expected: expected.to_owned(),
                actual: actual.to_owned(),
            }
        };
        self.contract_assertions.push(ContractAssertion {
            contract_name: name.to_owned(),
            status,
            details: details.to_owned(),
        });
        passed
    }

    pub fn record_latency(&mut self, stage: &str, duration_ms: u64) {
        self.latency_samples.push(LatencySample {
            stage: stage.to_owned(),
            duration_ms,
            at: chrono::Utc::now().to_rfc3339(),
        });
    }

    /// Derive the overall test outcome from all recorded evidence.
    ///
    /// Rules applied in priority order:
    /// 1. Any `ContractAssertion` with `AssertionResult::Failed` ⇒ `Failed`.
    /// 2. Any `ReportSection` with `ReportOutcome::Failed` ⇒ `Failed`.
    /// 3. Zero contract assertions AND zero sections ⇒ `Failed` with an
    ///    explicit "no assertions recorded" reason, so a scenario that never
    ///    asserted anything cannot masquerade as `Passed`.
    /// 4. Otherwise ⇒ `Passed`.
    pub fn build(self) -> E2EReport {
        let duration_ms = self.sections.iter().map(|s| s.duration_ms).sum();

        let any_assertion_failed = self
            .contract_assertions
            .iter()
            .any(|a| matches!(a.status, AssertionResult::Failed { .. }));
        let any_section_failed = self
            .sections
            .iter()
            .any(|s| matches!(s.status, ReportOutcome::Failed { .. }));
        let nothing_recorded = self.contract_assertions.is_empty() && self.sections.is_empty();

        let overall_outcome = if any_assertion_failed {
            ReportOutcome::Failed {
                reason: "one or more contract assertions failed".to_owned(),
            }
        } else if any_section_failed {
            ReportOutcome::Failed {
                reason: "one or more sections failed".to_owned(),
            }
        } else if nothing_recorded {
            ReportOutcome::Failed {
                reason: "no contract assertions recorded — scenario proved nothing".to_owned(),
            }
        } else {
            ReportOutcome::Passed
        };

        E2EReport {
            test_name: self.test_name,
            test_id: chrono::Utc::now().format("%Y%m%d%H%M%S").to_string(),
            started_at: self.started_at,
            duration_ms,
            outcome: overall_outcome,
            sections: self.sections,
            environment: EnvironmentSnapshot {
                pg_version: std::env::var("PG_VERSION").unwrap_or_else(|_| "unknown".to_owned()),
                qdrant_version: std::env::var("QDRANT_VERSION")
                    .unwrap_or_else(|_| "unknown".to_owned()),
                ollama_model: std::env::var("OLLAMA_MODEL")
                    .unwrap_or_else(|_| "nomic-embed-text".to_owned()),
                redis_version: std::env::var("REDIS_VERSION")
                    .unwrap_or_else(|_| "unknown".to_owned()),
            },
            contract_assertions: self.contract_assertions,
            degradation_events: self.degradation_events,
            latency_samples: self.latency_samples,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prove the previous bug: zero assertions used to yield `Passed`.
    /// After the fix this must yield `Failed` with an explicit "no assertions" reason.
    #[test]
    fn zero_assertions_and_zero_sections_yields_failed_not_passed() {
        let builder = ReportBuilder::new("empty-scenario");
        let report = builder.build();
        match report.outcome {
            ReportOutcome::Failed { reason } => {
                assert!(
                    reason.contains("no contract assertions"),
                    "reason must explain the absence of assertions, got: {reason}"
                );
            }
            other => panic!("expected Failed for zero-assertion scenario, got {other:?}"),
        }
    }

    /// A recorded `AssertionResult::Failed` must propagate to the overall outcome.
    #[test]
    fn failed_contract_assertion_yields_failed_outcome() {
        let mut builder = ReportBuilder::new("failing-scenario");
        builder.add_contract_assertion(ContractAssertion {
            contract_name: "graph_version_monotone".to_owned(),
            status: AssertionResult::Failed {
                expected: "version > 0".to_owned(),
                actual: "version = 0".to_owned(),
            },
            details: "graph never rebuilt".to_owned(),
        });
        let report = builder.build();
        assert!(
            matches!(report.outcome, ReportOutcome::Failed { .. }),
            "expected Failed when a contract assertion fails, got {:?}",
            report.outcome
        );
    }

    /// `assert_contract(passed=false)` must record a Failed assertion and return false.
    #[test]
    fn assert_contract_false_records_failed_and_returns_false() {
        let mut builder = ReportBuilder::new("computed-false");
        let result = builder.assert_contract(
            "version_monotone",
            false,
            "version > 0",
            "version = 0",
            "version was 0",
        );
        assert!(!result, "assert_contract must return the passed bool");
        let report = builder.build();
        assert!(
            matches!(report.outcome, ReportOutcome::Failed { .. }),
            "expected Failed outcome, got {:?}",
            report.outcome
        );
        assert_eq!(report.contract_assertions.len(), 1);
        assert!(matches!(
            report.contract_assertions[0].status,
            AssertionResult::Failed { .. }
        ));
    }

    /// `assert_contract(passed=true)` records a Passed assertion and yields overall Passed.
    #[test]
    fn assert_contract_true_records_passed_and_yields_passed_outcome() {
        let mut builder = ReportBuilder::new("computed-true");
        let result = builder.assert_contract(
            "version_monotone",
            true,
            "version > 0",
            "version = 5",
            "version was 5",
        );
        assert!(result, "assert_contract must return the passed bool");
        let report = builder.build();
        assert!(
            matches!(report.outcome, ReportOutcome::Passed),
            "expected Passed outcome, got {:?}",
            report.outcome
        );
    }

    /// A scenario with sections only (no explicit contract_assertions) still derives
    /// its outcome from those sections — existing behavior preserved.
    #[test]
    fn sections_only_with_passing_action_yields_passed() {
        let mut builder = ReportBuilder::new("sections-only");
        builder.push_action(
            "setup",
            ReportedAction {
                description: "seed data".to_owned(),
                status: AssertionResult::Passed,
                side_effects: vec![],
                duration_ms: 10,
            },
        );
        let report = builder.build();
        assert!(
            matches!(report.outcome, ReportOutcome::Passed),
            "expected Passed for section-only passing scenario, got {:?}",
            report.outcome
        );
    }

    /// A failing section still propagates to overall Failed even with no contract_assertions.
    #[test]
    fn failing_section_yields_failed_outcome() {
        let mut builder = ReportBuilder::new("failing-section");
        builder.push_action(
            "validation",
            ReportedAction {
                description: "check result".to_owned(),
                status: AssertionResult::Failed {
                    expected: "Ok".to_owned(),
                    actual: "Degraded".to_owned(),
                },
                side_effects: vec![],
                duration_ms: 5,
            },
        );
        let report = builder.build();
        assert!(
            matches!(report.outcome, ReportOutcome::Failed { .. }),
            "expected Failed for failing section, got {:?}",
            report.outcome
        );
    }
}
