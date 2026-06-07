use infrastructure::{
    DependencyFactory,
    logging::{ServiceLoggingConfig, init_service_logging},
};
use mcp_server::{
    McpServerApp,
    protocol::{DEFAULT_MCP_SERVER_ADDR, serve_http},
};
use retrieval::RetrievalConfig;
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

    let health_checker = DependencyFactory::build_health_checker_from_environment();
    let address = std::env::var("MCP_SERVER_ADDR")
        .unwrap_or_else(|_| DEFAULT_MCP_SERVER_ADDR.to_owned())
        .parse()?;

    let retrieval_config = RetrievalConfig {
        relevance_threshold: RetrievalConfig::relevance_threshold_from_env(),
        ..RetrievalConfig::default()
    };
    let app = McpServerApp::from_environment(retrieval_config)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error.to_string().into() })?
        .app;

    info!(
        service = "mcp-server",
        address = %address,
        registered_tools = ?app.registered_tools(),
        "mcp server listening"
    );
    serve_http(app, health_checker, address).await?;
    Ok(())
}
