use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, Utc};
use thiserror::Error;

use crate::audit::{
    MaintenanceAuditEvent, MaintenanceAuditSink, NoopMaintenanceAuditSink,
    RetirementProposalAuditEvent,
};
use crate::merge::SkillSnapshot;

/// Recorded usage event for retirement scoring.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageSample {
    pub skill_id: String,
    pub used_at: DateTime<Utc>,
    pub usage_count: u32,
}

/// Retirement proposal emitted as filesystem `.retired` marker.
#[derive(Debug, Clone, PartialEq)]
pub struct RetirementProposal {
    pub skill_id: String,
    pub retired_path: PathBuf,
    pub usage_score_per_month: f32,
}

/// Configures recency-based retirement scoring.
#[derive(Debug, Clone, PartialEq)]
pub struct RetirementConfig {
    pub score_threshold_per_month: f32,
    pub scoring_window_days: i64,
}

impl Default for RetirementConfig {
    fn default() -> Self {
        Self {
            score_threshold_per_month: 1.0,
            scoring_window_days: 90,
        }
    }
}

/// Scores stale skills and writes non-destructive `.retired` proposal markers.
pub struct RetirementProposalWriter<'s, S = NoopMaintenanceAuditSink>
where
    S: MaintenanceAuditSink,
{
    config: RetirementConfig,
    audit_sink: &'s S,
}

impl<'s> RetirementProposalWriter<'s, NoopMaintenanceAuditSink> {
    /// Creates a retirement workflow with explicit scoring settings.
    pub fn new(config: RetirementConfig) -> Self {
        Self {
            config,
            audit_sink: &NoopMaintenanceAuditSink,
        }
    }
}

impl<'s, S> RetirementProposalWriter<'s, S>
where
    S: MaintenanceAuditSink,
{
    /// Creates a retirement workflow with an explicit audit sink.
    pub fn with_audit_sink(config: RetirementConfig, audit_sink: &'s S) -> Self {
        Self { config, audit_sink }
    }

    /// Proposes retirement markers for skills below recency-weighted usage threshold.
    pub fn propose(
        &self,
        skills: &[SkillSnapshot],
        usage_samples: &[UsageSample],
        now: DateTime<Utc>,
    ) -> Result<Vec<RetirementProposal>, RetirementError> {
        let usage_index = usage_samples_by_skill_id(usage_samples);
        let mut proposals = Vec::new();
        for skill in skills {
            let usage_score = calculate_usage_score_per_month(
                usage_index
                    .get(&skill.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                now,
                self.config.scoring_window_days,
            );
            if usage_score >= self.config.score_threshold_per_month {
                continue;
            }
            let retired_path = retired_path_for_active_skill(&skill.source_path)?;
            let marker_body = render_retired_marker(&skill.id, usage_score, now);
            let mut retired_marker_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&retired_path)
                .map_err(|error| RetirementError::RetiredMarkerWriteFailure {
                    path: retired_path.display().to_string(),
                    message: error.to_string(),
                })?;
            retired_marker_file
                .write_all(marker_body.as_bytes())
                .map_err(|error| RetirementError::RetiredMarkerWriteFailure {
                    path: retired_path.display().to_string(),
                    message: error.to_string(),
                })?;
            retired_marker_file.flush().map_err(|error| {
                RetirementError::RetiredMarkerWriteFailure {
                    path: retired_path.display().to_string(),
                    message: error.to_string(),
                }
            })?;
            let proposal = RetirementProposal {
                skill_id: skill.id.clone(),
                retired_path,
                usage_score_per_month: usage_score,
            };
            self.emit_retirement_proposal_audit(skill, now, &proposal)?;
            proposals.push(proposal);
        }
        Ok(proposals)
    }

    fn emit_retirement_proposal_audit(
        &self,
        skill: &SkillSnapshot,
        now: DateTime<Utc>,
        proposal: &RetirementProposal,
    ) -> Result<(), RetirementError> {
        let correlation_id = format!("maintenance.retirement_proposal:{}", skill.id);
        let audit_event =
            MaintenanceAuditEvent::RetirementProposalWritten(RetirementProposalAuditEvent {
                correlation_id,
                happened_at: now,
                skill_id: proposal.skill_id.clone(),
                source_path: skill.source_path.clone(),
                proposal_path: proposal.retired_path.clone(),
                usage_score_per_month: proposal.usage_score_per_month,
            });
        self.audit_sink
            .emit(audit_event)
            .map_err(|error| RetirementError::AuditEmissionFailure(error.to_string()))
    }
}

#[derive(Debug, Error)]
pub enum RetirementError {
    #[error("skill `{0}` is not an active skill file and cannot be retired")]
    InvalidActiveSkillPath(String),
    #[error("retired path `{path}` resolves outside active skill root `{skill_root}`")]
    RetiredPathOutsideSkillRoot { path: String, skill_root: String },
    #[error("failed creating retired marker `{path}`: {message}")]
    RetiredMarkerWriteFailure { path: String, message: String },
    #[error("failed emitting retirement proposal audit event: {0}")]
    AuditEmissionFailure(String),
}

impl RetirementError {
    /// Maps retirement workflow failures to stable reason codes.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::InvalidActiveSkillPath(_) => "retire_invalid_active_skill_path",
            Self::RetiredPathOutsideSkillRoot { .. } => "retire_path_outside_skill_root",
            Self::RetiredMarkerWriteFailure { .. } => "retire_marker_write_failed",
            Self::AuditEmissionFailure(_) => "retire_audit_emission_failed",
        }
    }
}

fn canonical_active_skill_root(active_path: &Path) -> Result<PathBuf, RetirementError> {
    let parent = active_path.parent().ok_or_else(|| {
        RetirementError::InvalidActiveSkillPath(active_path.display().to_string())
    })?;
    if !parent.is_absolute() {
        return Err(RetirementError::InvalidActiveSkillPath(format!(
            "{} (parent must be absolute)",
            active_path.display()
        )));
    }
    let canonical_parent = parent.canonicalize().map_err(|error| {
        RetirementError::InvalidActiveSkillPath(format!(
            "{} (cannot canonicalize parent: {error})",
            active_path.display()
        ))
    })?;
    if !canonical_parent.is_dir() {
        return Err(RetirementError::InvalidActiveSkillPath(format!(
            "{} (parent must resolve to directory)",
            active_path.display()
        )));
    }
    Ok(canonical_parent)
}

fn ensure_path_is_within_skill_root(
    candidate_path: &Path,
    canonical_skill_root: &Path,
) -> Result<(), RetirementError> {
    if candidate_path.starts_with(canonical_skill_root) {
        return Ok(());
    }
    Err(RetirementError::RetiredPathOutsideSkillRoot {
        path: candidate_path.display().to_string(),
        skill_root: canonical_skill_root.display().to_string(),
    })
}

fn usage_samples_by_skill_id(samples: &[UsageSample]) -> HashMap<String, Vec<UsageSample>> {
    let mut index = HashMap::new();
    for sample in samples {
        index
            .entry(sample.skill_id.clone())
            .or_insert_with(Vec::new)
            .push(sample.clone());
    }
    index
}

fn calculate_usage_score_per_month(
    usage_samples: &[UsageSample],
    now: DateTime<Utc>,
    scoring_window_days: i64,
) -> f32 {
    let window_start = now - Duration::days(scoring_window_days);
    let weighted_usage = usage_samples
        .iter()
        .filter(|sample| sample.used_at >= window_start)
        .map(|sample| {
            let age_days = (now - sample.used_at).num_days().max(0) as f32;
            let recency_weight = 1.0 - (age_days / scoring_window_days as f32);
            sample.usage_count as f32 * recency_weight.max(0.0)
        })
        .sum::<f32>();
    let months = (scoring_window_days as f32 / 30.0).max(1.0);
    weighted_usage / months
}

fn retired_path_for_active_skill(active_path: &Path) -> Result<PathBuf, RetirementError> {
    let file_name = active_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            RetirementError::InvalidActiveSkillPath(active_path.display().to_string())
        })?;
    if !file_name.eq_ignore_ascii_case("SKILL.md") {
        return Err(RetirementError::InvalidActiveSkillPath(
            active_path.display().to_string(),
        ));
    }
    let canonical_skill_root = canonical_active_skill_root(active_path)?;
    let retired_path = canonical_skill_root.join("SKILL.md.retired");
    if retired_path.exists() {
        let canonical_retired_path = retired_path.canonicalize().map_err(|error| {
            RetirementError::RetiredMarkerWriteFailure {
                path: retired_path.display().to_string(),
                message: error.to_string(),
            }
        })?;
        ensure_path_is_within_skill_root(&canonical_retired_path, &canonical_skill_root)?;
    }
    Ok(retired_path)
}

fn render_retired_marker(skill_id: &str, usage_score_per_month: f32, now: DateTime<Utc>) -> String {
    format!(
        "---\norigin: retirement_proposal\nskill_id: {skill_id}\nusage_score_per_month: {:.3}\nproposed_at: {}\nstatus: proposed\n---\n",
        usage_score_per_month,
        now.to_rfc3339()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_score_drops_to_zero_without_recent_activity() {
        let now = Utc::now();
        let score = calculate_usage_score_per_month(
            &[UsageSample {
                skill_id: "s1".to_owned(),
                used_at: now - Duration::days(180),
                usage_count: 5,
            }],
            now,
            90,
        );
        assert_eq!(score, 0.0);
    }
}
