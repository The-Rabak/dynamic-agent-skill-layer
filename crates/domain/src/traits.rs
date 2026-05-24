use async_trait::async_trait;

use crate::{
    errors::{EmbeddingError, ExtractionError, ScopeError},
    types::{ExtractionResult, ScopeDescriptor, ScoredSkill, SessionTranscript},
};

#[async_trait]
pub trait EmbeddingService: Send + Sync {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}

#[async_trait]
pub trait TranscriptSkillExtractionService: Send + Sync {
    async fn extract(
        &self,
        transcript: &SessionTranscript,
    ) -> Result<ExtractionResult, ExtractionError>;
}

#[async_trait]
pub trait ScopeResolver: Send + Sync {
    async fn resolve(&self, repo_path: Option<&str>) -> Result<Vec<ScopeDescriptor>, ScopeError>;
}

pub trait ContextCompiler: Send + Sync {
    fn compile(&self, skills: &[ScoredSkill], prompt: &str) -> String;
}
