fn main() {
    // Bad fixture: silently falls back to hardcoded test-infrastructure port 15432
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://admin:secret@localhost:15432/skills".to_string());
    println!("Maintenance connecting to: {}", db_url);
}
