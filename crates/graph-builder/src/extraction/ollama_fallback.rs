use domain::{
    DomainId, ExtractedSkillCandidate, SessionTranscript, SubunitType, TranscriptEntry,
    TranscriptSkillExtractionService,
};
use infrastructure::{OllamaExtractionConfig, OllamaExtractor};

use crate::extraction::ExtractedSubunit;

/// Produces fallback subunits by first trying the Ollama extraction provider and then
/// degrading to an explicit unavailable-provider result.
pub fn extract_with_ollama_fallback(markdown: &str) -> Vec<ExtractedSubunit> {
    extract_with_provider(markdown, &OllamaFallbackProvider)
        .unwrap_or_else(|error| explicit_unavailable_subunits(markdown, &error))
}

trait FallbackExtractionProvider {
    fn extract_candidates(&self, markdown: &str) -> Result<Vec<ExtractedSkillCandidate>, String>;
}

struct OllamaFallbackProvider;

impl FallbackExtractionProvider for OllamaFallbackProvider {
    fn extract_candidates(&self, markdown: &str) -> Result<Vec<ExtractedSkillCandidate>, String> {
        let config = ollama_config_from_environment()?;
        let extractor = OllamaExtractor::new(reqwest::Client::new(), config)
            .map_err(|error| format!("failed to initialize Ollama extraction provider: {error}"))?;
        let transcript = fallback_transcript(markdown);
        let extraction_result = run_ollama_extraction(&extractor, &transcript)?;
        Ok(extraction_result.candidates)
    }
}

fn extract_with_provider(
    markdown: &str,
    provider: &impl FallbackExtractionProvider,
) -> Result<Vec<ExtractedSubunit>, String> {
    let candidates = provider.extract_candidates(markdown)?;
    provider_candidates_to_subunits(markdown, candidates)
}

fn provider_candidates_to_subunits(
    markdown: &str,
    candidates: Vec<ExtractedSkillCandidate>,
) -> Result<Vec<ExtractedSubunit>, String> {
    let candidate = candidates
        .into_iter()
        .next()
        .ok_or_else(|| "Ollama fallback provider returned zero candidates".to_owned())?;
    let summary_content = if candidate.description.trim().is_empty() {
        first_extractable_line(markdown).to_owned()
    } else {
        candidate.description.trim().to_owned()
    };

    let mut subunits = vec![ExtractedSubunit {
        kind: SubunitType::Summary,
        title: "Fallback summary".to_owned(),
        content: summary_content,
    }];
    append_candidate_subunits(
        &mut subunits,
        SubunitType::Procedure,
        "Fallback procedure",
        &candidate.procedures,
    );
    append_candidate_subunits(
        &mut subunits,
        SubunitType::Convention,
        "Fallback convention",
        &candidate.conventions,
    );
    append_candidate_subunits(
        &mut subunits,
        SubunitType::Asset,
        "Fallback asset",
        &candidate.assets,
    );
    Ok(subunits)
}

fn append_candidate_subunits(
    subunits: &mut Vec<ExtractedSubunit>,
    kind: SubunitType,
    title_prefix: &str,
    source_lines: &[String],
) {
    for (index, content) in source_lines
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .enumerate()
    {
        subunits.push(ExtractedSubunit {
            kind,
            title: format!("{title_prefix} {}", index + 1),
            content: content.to_owned(),
        });
    }
}

fn explicit_unavailable_subunits(markdown: &str, error: &str) -> Vec<ExtractedSubunit> {
    vec![
        ExtractedSubunit {
            kind: SubunitType::Summary,
            title: "Fallback summary".to_owned(),
            content: first_extractable_line(markdown).to_owned(),
        },
        ExtractedSubunit {
            kind: SubunitType::Evidence,
            title: "Fallback provider unavailable".to_owned(),
            content: format!("Ollama fallback unavailable: {error}"),
        },
    ]
}

fn first_extractable_line(markdown: &str) -> &str {
    markdown
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .unwrap_or("No extractable content")
}

fn fallback_transcript(markdown: &str) -> SessionTranscript {
    SessionTranscript {
        session_id: DomainId::new_unchecked("graph-builder-ollama-fallback"),
        entries: vec![TranscriptEntry {
            speaker: "system".to_owned(),
            content: markdown.to_owned(),
        }],
    }
}

fn run_ollama_extraction(
    extractor: &OllamaExtractor,
    transcript: &SessionTranscript,
) -> Result<domain::ExtractionResult, String> {
    let extraction_future = extractor.extract(transcript);
    match tokio::runtime::Handle::try_current() {
        Ok(runtime_handle) => match runtime_handle.runtime_flavor() {
            tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| runtime_handle.block_on(extraction_future))
            }
            tokio::runtime::RuntimeFlavor::CurrentThread => {
                return Err(
                    "Ollama fallback extraction requires a multi-thread Tokio runtime when running inside async contexts"
                        .to_owned(),
                );
            }
            _ => {
                return Err(
                    "Ollama fallback extraction encountered an unsupported Tokio runtime flavor"
                        .to_owned(),
                );
            }
        },
        Err(_) => {
            let runtime = tokio::runtime::Runtime::new()
                .map_err(|error| format!("failed to create fallback runtime: {error}"))?;
            runtime.block_on(extraction_future)
        }
    }
    .map_err(|error| format!("Ollama fallback extraction failed: {error}"))
}

fn ollama_config_from_environment() -> Result<OllamaExtractionConfig, String> {
    let mut config = OllamaExtractionConfig::default();
    if let Ok(endpoint) = std::env::var("OLLAMA_EXTRACTION_ENDPOINT") {
        config.endpoint = endpoint;
    }
    if let Ok(model) = std::env::var("OLLAMA_EXTRACTION_MODEL") {
        config.model = model;
    }
    if let Ok(timeout_ms) = std::env::var("OLLAMA_EXTRACTION_TIMEOUT_MS") {
        config.timeout_ms = timeout_ms.parse().map_err(|error| {
            format!("invalid OLLAMA_EXTRACTION_TIMEOUT_MS value for fallback: {error}")
        })?;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct StubFallbackProvider {
        invocations: Arc<AtomicUsize>,
        next_result: Result<Vec<ExtractedSkillCandidate>, String>,
    }

    impl FallbackExtractionProvider for StubFallbackProvider {
        fn extract_candidates(
            &self,
            _markdown: &str,
        ) -> Result<Vec<ExtractedSkillCandidate>, String> {
            self.invocations.fetch_add(1, Ordering::SeqCst);
            self.next_result.clone()
        }
    }

    #[test]
    fn uses_provider_candidates_for_structured_subunits() {
        let invocations = Arc::new(AtomicUsize::new(0));
        let provider = StubFallbackProvider {
            invocations: invocations.clone(),
            next_result: Ok(vec![ExtractedSkillCandidate {
                name: "skill-fallback".to_owned(),
                description: "Provider extracted summary".to_owned(),
                tags: vec![],
                procedures: vec!["Step one".to_owned()],
                conventions: vec!["Stay deterministic".to_owned()],
                assets: vec!["docs/skill.md".to_owned()],
                confidence: 0.8,
            }]),
        };

        let subunits = extract_with_provider("# heading\nStatic line", &provider)
            .expect("provider-backed extraction should succeed");

        assert_eq!(invocations.load(Ordering::SeqCst), 1);
        assert_eq!(subunits.len(), 4);
        assert_eq!(subunits[0].kind, SubunitType::Summary);
        assert_eq!(subunits[0].content, "Provider extracted summary");
        assert_eq!(subunits[1].kind, SubunitType::Procedure);
        assert_eq!(subunits[2].kind, SubunitType::Convention);
        assert_eq!(subunits[3].kind, SubunitType::Asset);
    }

    #[test]
    fn explicit_unavailable_shape_is_used_when_provider_fails() {
        let invocations = Arc::new(AtomicUsize::new(0));
        let provider = StubFallbackProvider {
            invocations: invocations.clone(),
            next_result: Err("connection refused".to_owned()),
        };

        let fallback_subunits = extract_with_provider("# heading\nDeterministic line", &provider)
            .unwrap_or_else(|error| {
                explicit_unavailable_subunits("# heading\nDeterministic line", &error)
            });

        assert_eq!(invocations.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_subunits.len(), 2);
        assert_eq!(fallback_subunits[0].kind, SubunitType::Summary);
        assert_eq!(fallback_subunits[0].content, "Deterministic line");
        assert_eq!(fallback_subunits[1].kind, SubunitType::Evidence);
        assert!(
            fallback_subunits[1]
                .content
                .contains("Ollama fallback unavailable: connection refused")
        );
    }
}
