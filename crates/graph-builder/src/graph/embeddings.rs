/// Embeds extracted text into deterministic vectors for durable graph storage.
pub trait EmbeddingGenerator {
    fn embed_text(&self, text: &str) -> Vec<f32>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicEmbeddingGenerator;

impl EmbeddingGenerator for DeterministicEmbeddingGenerator {
    fn embed_text(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0_f32; 8];
        for token in text
            .to_ascii_lowercase()
            .split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            let hash = blake3::hash(token.as_bytes());
            let index = usize::from(hash.as_bytes()[0]) % vector.len();
            vector[index] += 1.0;
        }
        vector
    }
}
