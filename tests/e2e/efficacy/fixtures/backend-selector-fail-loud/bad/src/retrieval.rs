// Bad fixture: QdrantHybrid arm silently clones dense results (the original bug).
#[derive(Debug, Clone, PartialEq)]
pub enum RetrievalBackend {
    SnapshotDense,
    QdrantHybrid,
}

pub struct SearchError(pub String);

pub fn search(
    backend: &RetrievalBackend,
    query: &str,
) -> Result<Vec<String>, SearchError> {
    let dense_results = vec![format!("dense:{}", query)];
    match backend {
        RetrievalBackend::SnapshotDense => Ok(dense_results),
        // silent passthrough: caller thinks it got hybrid results but gets dense
        RetrievalBackend::QdrantHybrid => Ok(dense_results.clone()),
    }
}
