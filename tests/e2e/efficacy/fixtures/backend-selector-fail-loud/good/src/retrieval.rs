// Good fixture: QdrantHybrid arm returns Err instead of silently cloning dense results.
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
        RetrievalBackend::QdrantHybrid => Err(SearchError(
            "QdrantHybrid is not yet implemented; use SnapshotDense".to_string(),
        )),
    }
}
