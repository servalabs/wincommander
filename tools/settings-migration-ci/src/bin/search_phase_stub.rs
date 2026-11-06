fn main() {
    let args: Vec<_> = std::env::args().collect();
    if std::env::var_os("WINCMD_TEST_SELECTED").is_some() {
        println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:selected");
        return;
    }
    match args.get(2).map(String::as_str) {
        Some("stage") => println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:staged reconciliation=pending old-cache=retained"),
        Some("verify-staged") => println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:staged old-cache=retained"),
        _ => std::process::exit(1),
    }
}
