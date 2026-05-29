use std::sync::Arc;

use async_trait::async_trait;
use criterion::{Criterion, criterion_group, criterion_main};
use domain::{DomainId, EmbeddingError, EmbeddingService, LifecycleStatus, ScopeType, Skill, SkillStatus, Subunit, SubunitType};
use mcp_server::{build_seeded_server, tools::compile_context::CompileContextRequest};
use retrieval::{RetrievalConfig, SeededGraph, SeededSkill};

/// Deterministic embedding service that returns fixed-dimension vectors instantly.
/// No network calls — this isolates retrieval + compilation latency.
struct MockEmbeddingService;

#[async_trait]
impl EmbeddingService for MockEmbeddingService {
    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        // Deterministic hash-based embedding: stable across runs, no I/O.
        let mut vec = vec![0.0_f32; 768];
        let bytes = text.as_bytes();
        for (idx, cell) in vec.iter_mut().enumerate() {
            *cell = ((bytes.iter().map(|b| *b as u64).sum::<u64>() + idx as u64) % 1000) as f32
                / 1000.0;
        }
        Ok(vec)
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_text(text).await?);
        }
        Ok(results)
    }
}

fn build_seeded_skill(id: &str, name: &str, scope: ScopeType, embedding: Vec<f32>) -> SeededSkill {
    SeededSkill {
        skill: Skill {
            id: DomainId::new_unchecked(id),
            name: name.to_owned(),
            description: format!("Description for {name}"),
            scope,
            status: SkillStatus::Ready,
            lifecycle: LifecycleStatus::Active,
            tags: vec!["benchmark".to_owned()],
            subunit_ids: vec![],
            community_id: None,
        },
        scope_id: match scope {
            ScopeType::Project => "project".to_owned(),
            ScopeType::Global => "global".to_owned(),
            ScopeType::Team => "team".to_owned(),
        },
        source_paths: vec![],
        embedding,
        subunits: vec![Subunit {
            id: DomainId::new_unchecked(&format!("{id}-sub-0")),
            skill_id: DomainId::new_unchecked(id),
            kind: SubunitType::Procedure,
            title: "Procedure".to_owned(),
            content: format!("Procedure content for {name}"),
            lifecycle: LifecycleStatus::Active,
        }],
        prior: 0.5,
        community_boost: 0.0,
    }
}

fn build_graph(skill_count: usize) -> SeededGraph {
    let mut skills = Vec::with_capacity(skill_count);
    for idx in 0..skill_count {
        let scope = if idx % 2 == 0 {
            ScopeType::Project
        } else {
            ScopeType::Global
        };
        let embedding = vec![(idx % 1000) as f32 / 1000.0; 768];
        skills.push(build_seeded_skill(
            &format!("skill-{idx:04}"),
            &format!("skill-{idx:04}"),
            scope,
            embedding,
        ));
    }
    SeededGraph::new(skills, 1)
}

fn bench_compile_context(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    for size in [100, 1_000, 5_000] {
        let graph = build_graph(size);
        let app = build_seeded_server(
            Arc::new(MockEmbeddingService),
            graph,
            RetrievalConfig::default(),
            None,
        );

        let request = CompileContextRequest {
            prompt: "how do I read a file in rust".to_owned(),
            session_id: "bench-session".to_owned(),
            repo_path: "/tmp/bench-repo".to_owned(),
        };

        c.bench_function(&format!("compile_context_{size}_skills"), |b| {
            b.to_async(&rt)
                .iter(|| async { app.compile_context(request.clone()).await })
        });
    }
}

criterion_group!(benches, bench_compile_context);
criterion_main!(benches);
