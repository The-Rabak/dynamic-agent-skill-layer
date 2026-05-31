pub mod dual_scope;
pub mod fusion;
pub mod graph_search;
pub mod orchestrator;
pub mod qdrant_search;
pub mod scope_resolution;
pub mod scoring;

pub use dual_scope::{
    ScopedSearchFailure, ScopedSearchResult, run_project_and_global_concurrently,
    search_scopes_concurrently,
};
pub use fusion::{FusedCandidate, ScopeRanking, mmr_select, weighted_reciprocal_rank_fusion};
pub use graph_search::{GraphHit, SubunitProjection, search_graph};
pub use orchestrator::{
    RescueCue, RetrievalConfig, RetrievalOrchestrator, RetrievalOutcome, RetrievalSnapshot,
    RetrievedSkill, RetrievedSubunit, SeededSkill, SkillRetriever,
};
pub use qdrant_search::{QdrantHit, search_qdrant};
pub use scope_resolution::{DualScopeResolver, ScopeResolutionOutcome};
pub use scoring::{ScoreComponents, ScoringWeights, score_eq3};
