use infrastructure::{
    DependencyFactory,
    logging::{ServiceLoggingConfig, init_service_logging},
};
use mcp_server::{
    McpServerApp,
    protocol::{DEFAULT_MCP_SERVER_ADDR, serve_http},
};
use retrieval::{RetrievalBackend, RetrievalConfig};
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

    let mut health_checker = DependencyFactory::build_health_checker_from_environment();
    let address = std::env::var("MCP_SERVER_ADDR")
        .unwrap_or_else(|_| DEFAULT_MCP_SERVER_ADDR.to_owned())
        .parse()?;

    // All ranking levers are env-overridable (fail-loud) for operational
    // retuning without a redeploy and for the #210 retrieval-quality sweep,
    // which measures each lever on the REAL running server by rebooting it per
    // config. Absent variables fall back to the calibrated defaults.
    let retrieval_config = RetrievalConfig::from_env();
    // Capture the backend label before moving retrieval_config into from_environment.
    let backend_label = match retrieval_config.backend {
        RetrievalBackend::SnapshotDense => "snapshot_dense",
        RetrievalBackend::SnapshotHybrid => "snapshot_hybrid",
        RetrievalBackend::QdrantHybrid => "qdrant_hybrid",
    };
    let live = McpServerApp::from_environment(retrieval_config)
        .await
        .map_err(|error| -> Box<dyn std::error::Error> { error.to_string().into() })?;

    // Surface the active embedding arm on /health so agents can discover which
    // vector space produced find_skill results — agent-native parity (#239).
    // Sources the boot-discovered model name + dimension from EmbeddingModelInfo
    // (set by build_live_server after discover_dimension) and the resolved
    // collection name from the Qdrant adapter config.
    // Future: once #228's embedding_model_metadata row is populated by the first
    // graph rebuild, that row is the canonical source; the boot value is used until
    // then (and is always correct for the currently active arm).
    health_checker = health_checker.with_static_component(
        "embedding_arm",
        true,
        format!(
            "model={} dim={} collection={}",
            live.embedding_model_info.model_name,
            live.embedding_model_info.dimension,
            live.qdrant_adapter.config.collection_name,
        ),
    );

    // Surface the active retrieval backend on /health so agents can tell which
    // candidate-generation path produced find_skill results (#255 P2-C/D).
    // Parsed fail-loud at boot (RetrievalConfig::from_env()); the value here
    // reflects what the real server is running, not a stale env snapshot.
    health_checker = health_checker.with_static_component(
        "retrieval_backend",
        true,
        format!("backend={backend_label}"),
    );

    let app = live.app;

    info!(
        service = "mcp-server",
        address = %address,
        registered_tools = ?app.registered_tools(),
        "mcp server listening"
    );
    serve_http(app, health_checker, address).await?;
    Ok(())
}
