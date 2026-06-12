// Good fixture: migration 012 is wired into BOTH a const AND the MIGRATIONS array.
const M001: &str = include_str!("../migrations/001_initial_schema.sql");
const M011: &str = include_str!("../migrations/011_add_skill_embeddings.sql");
const M012: &str = include_str!("../migrations/012_add_skill_tags_index.sql");

pub static MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial_schema", M001),
    ("011_add_skill_embeddings", M011),
    ("012_add_skill_tags_index", M012),
];
