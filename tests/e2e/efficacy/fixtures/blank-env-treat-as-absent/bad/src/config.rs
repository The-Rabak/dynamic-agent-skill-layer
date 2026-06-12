/// Return the configured Ollama embedding model name.
///
/// BUG: only handles Err (absent), not Ok("") (blank from docker-compose).
pub fn embed_model() -> String {
    std::env::var("OLLAMA_EMBED_MODEL")
        .unwrap_or_else(|_| "qwen3-embedding:4b".to_string())
}
