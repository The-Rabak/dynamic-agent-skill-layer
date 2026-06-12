fn main() {
    // Good fixture: fail-loud pattern applied; no hardcoded fallback URL
    let db_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    println!("Maintenance connecting to: {}", db_url);
}
