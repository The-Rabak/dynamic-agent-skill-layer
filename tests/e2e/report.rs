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
    DbRowInserted(String),
    EventPublished(String),
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
            if matches!(action_outcome, ReportOutcome::Failed { .. }) {
                existing.status = action_outcome;
            } else if matches!(existing.status, ReportOutcome::Passed)
                && matches!(action_outcome, ReportOutcome::Skipped { .. })
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

    pub fn record_degradation_event(
        &mut self,
        service: &str,
        recovered: bool,
        reason: &str,
    ) {
        self.degradation_events.push(DegradationEvent {
            service: service.to_owned(),
            at: chrono::Utc::now().to_rfc3339(),
            recovered,
            reason: reason.to_owned(),
        });
    }

    pub fn add_contract_assertion(&mut self, assertion: ContractAssertion) {
        self.contract_assertions.push(assertion);
    }

    pub fn record_latency(&mut self, stage: &str, duration_ms: u64) {
        self.latency_samples.push(LatencySample {
            stage: stage.to_owned(),
            duration_ms,
            at: chrono::Utc::now().to_rfc3339(),
        });
    }

    pub fn build(self) -> E2EReport {
        let duration_ms = self.sections.iter().map(|s| s.duration_ms).sum();
        let overall_outcome = if self
            .sections
            .iter()
            .any(|s| matches!(s.status, ReportOutcome::Failed { .. }))
        {
            ReportOutcome::Failed {
                reason: "one or more sections failed".to_owned(),
            }
        } else {
            ReportOutcome::Passed
        };

        E2EReport {
            test_name: self.test_name,
            test_id: chrono::Utc::now()
                .format("%Y%m%d%H%M%S")
                .to_string(),
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