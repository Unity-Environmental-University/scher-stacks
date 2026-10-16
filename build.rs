// build.rs — one job: make cargo rebuild when a migration is added or edited.
//
// `sqlx::migrate!("./migrations")` (main.rs) embeds the .sql files into the binary at compile
// time. On stable Rust the macro cannot tell cargo it read that directory, so without this a new
// migrations/000N_*.sql leaves the crate "Fresh" and the old binary ships without it. Checked
// 2026-10-16: adding a file to migrations/ did not rebuild. sqlx's own docs recommend this file.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
