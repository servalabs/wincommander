fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(2).map(String::as_str) {
        Some("stage") => println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:staged reconciliation=pending old-cache=retained"),
        Some("verify-staged") => println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:staged old-cache=retained"),
        _ => std::process::exit(1),
    }
}
