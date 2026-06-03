use infrastructure::{
    DependencyFactory,
    logging::{ServiceLoggingConfig, init_service_logging},
};
use mcp_server::{
    McpServerApp,
    protocol::{DEFAULT_MCP_SERVER_ADDR, serve_http},
};
use retrieval::{RetrievalConfig, RetrievalSnapshot};
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

    let redis_client = DependencyFactory::build_redis_client_from_environment();
    let health_checker = DependencyFactory::build_health_checker_from_environment();
    let address = std::env::var("MCP_SERVER_ADDR")
        .unwrap_or_else(|_| DEFAULT_MCP_SERVER_ADDR.to_owned())
        .parse()?;

    // TODO(remove-after-v1.5-green): temporary rollback switch back to the empty
    // seeded graph. Remove on first green CI on `main` once the live boot path
    // is proven in deployment. Default is `live` so production boots the real graph.
    let retrieval_mode = std::env::var("MCP_RETRIEVAL_MODE").unwrap_or_else(|_| "live".to_owned());

    let app = if retrieval_mode == "seeded" {
        let embedding_service = DependencyFactory::build_embedding_service_from_environment()?;
        McpServerApp::with_explicit_graph(
            embedding_service,
            RetrievalSnapshot::new(Vec::new(), 0),
            RetrievalConfig::default(),
            redis_client,
        )
    } else {
        McpServerApp::from_environment(RetrievalConfig::default())
            .await
            .map_err(|error| -> Box<dyn std::error::Error> { error.to_string().into() })?
            .app
    };

    info!(
        service = "mcp-server",
        address = %address,
        retrieval_mode = %retrieval_mode,
        registered_tools = ?app.registered_tools(),
        "mcp server listening"
    );
    serve_http(app, health_checker, address).await?;
    Ok(())
}
