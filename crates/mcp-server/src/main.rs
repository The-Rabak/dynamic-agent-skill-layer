use infrastructure::{
    DependencyFactory,
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

    let embedding_service = DependencyFactory::build_embedding_service_from_environment()?;
    let graph = SeededGraph::new(Vec::new(), 0);
    let redis_client = DependencyFactory::build_redis_client_from_environment();
    let app = build_seeded_server(embedding_service, graph, RetrievalConfig::default(), redis_client);
    let health_checker = DependencyFactory::build_health_checker_from_environment();
    let address = std::env::var("MCP_SERVER_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3001".to_owned())
        .parse()?;

    info!(
        service = "mcp-server",
        address = %address,
        registered_tools = ?app.registered_tools(),
        "mcp server listening"
    );
    serve_http(app, health_checker, address).await?;
    Ok(())
}