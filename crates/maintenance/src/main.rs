/// Maintenance worker process entrypoint.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    maintenance::run_maintenance_worker_from_environment()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(())
}
