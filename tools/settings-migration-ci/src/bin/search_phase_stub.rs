fn main() {
    let args: Vec<_> = std::env::args().collect();
    match args.get(2).map(String::as_str) {
        Some("inspect") => println!("WINCOMMANDER_SEARCH_TRANSACTION_OUTCOME:none"),
        Some("prepare") => println!("WINCOMMANDER_SEARCH_TRANSACTION_OUTCOME:replacement-pending reconciliation=pending"),
        Some("commit") => println!("WINCOMMANDER_SEARCH_TRANSACTION_OUTCOME:committed"),
        _ => std::process::exit(1),
    }
}
