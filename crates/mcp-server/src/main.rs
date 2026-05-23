use std::sync::Arc;

use domain::EmbeddingService;
use infrastructure::{
    OllamaEmbeddingConfig, OllamaEmbeddingService,
    logging::{ServiceLoggingConfig, init_service_logging},
};
use mcp_server::{build_seeded_server, protocol::serve_http};
use retrieval::{RetrievalConfig, SeededGraph};
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment = std::env::var("APP_ENV")
        .or_else(|_| std::env::var("ENVIRONMENT"))
        .unwrap_or_else(|_| "local".to_owned());
    init_service_logging(ServiceLoggingConfig::new(
        "mcp-server",
        env!("CARGO_PKG_VERSION"),
        environment,
        "info",
    ))?;

    let embedding_service = build_default_embedding_service()?;
    let graph = SeededGraph::new(Vec::new(), 0);
    let app = build_seeded_server(embedding_service, graph, RetrievalConfig::default());
    let address = std::env::var("MCP_SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3001".to_owned())
        .parse()?;

    info!(
        service = "mcp-server",
        address = %address,
        registered_tools = ?app.registered_tools(),
        "mcp server listening"
    );
    serve_http(app, address).await?;
    Ok(())
}

fn build_default_embedding_service()
-> Result<Arc<impl EmbeddingService>, Box<dyn std::error::Error>> {
    let service =
        OllamaEmbeddingService::new(reqwest::Client::new(), OllamaEmbeddingConfig::default())
            .map_err(|error| std::io::Error::other(error.to_string()))?;

    Ok(Arc::new(service))
}
