/// Return the configured Ollama embedding model name.
///
/// Treats both missing (Err) and blank Ok("") as absent — docker-compose
/// ${OLLAMA_EMBED_MODEL:-} emits Ok("") when the host variable is unset.
pub fn embed_model() -> String {
    std::env::var("OLLAMA_EMBED_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "qwen3-embedding:4b".to_string())
}
