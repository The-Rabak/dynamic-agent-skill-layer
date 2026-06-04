/// Test-support only — deterministic 768-dim embeddings for tests without a live Ollama.
/// Gated behind `test-utils` feature to prevent accidental production use.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicEmbeddingService;

#[cfg(any(test, feature = "test-utils"))]
impl DeterministicEmbeddingService {
    /// Produces a deterministic 768-dim vector from text without any network calls.
    ///
    /// Each alphanumeric token is hashed with blake3; the first two bytes form a
    /// `u16` index modulo 768, incrementing that bucket. The result is L2-normalized
    /// (guarded against a zero-norm vector) so cosine similarity is well-defined.
    fn deterministic_768(text: &str) -> Vec<f32> {
        let mut buckets = vec![0.0_f32; 768];
        for token in text
            .to_ascii_lowercase()
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            let hash = blake3::hash(token.as_bytes());
            let bytes = hash.as_bytes();
            let index = usize::from(u16::from_le_bytes([bytes[0], bytes[1]])) % 768;
            buckets[index] += 1.0;
        }

        let norm: f32 = buckets.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            for bucket in &mut buckets {
                *bucket /= norm;
            }
        }
        buckets
    }
}

#[cfg(any(test, feature = "test-utils"))]
#[async_trait::async_trait]
impl domain::EmbeddingService for DeterministicEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, domain::EmbeddingError> {
        Ok(Self::deterministic_768(text))
    }

    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, domain::EmbeddingError> {
        if texts.is_empty() {
            return Err(domain::EmbeddingError::InvalidInput(
                "batch input must contain at least one text".to_owned(),
            ));
        }
        Ok(texts.iter().map(|text| Self::deterministic_768(text)).collect())
    }
}
