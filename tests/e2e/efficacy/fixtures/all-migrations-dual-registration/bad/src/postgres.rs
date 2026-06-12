// Bad fixture: migration 012 const is added but NOT added to the MIGRATIONS array.
// A generic agent would add only the const or only the array entry.
const M001: &str = include_str!("../migrations/001_initial_schema.sql");
const M011: &str = include_str!("../migrations/011_add_skill_embeddings.sql");
const M012: &str = include_str!("../migrations/012_add_skill_tags_index.sql");

pub static MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial_schema", M001),
    ("011_add_skill_embeddings", M011),
    // M012 is intentionally NOT added to MIGRATIONS — simulates the half-wired bug
];
