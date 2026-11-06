// Synthetic CI-only fixture: never linked into the desktop application.
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};
#[cfg(all(windows, feature = "experimental-encrypted-search"))]
use wincmd_search::{types::{ContentQuery, IndexConfig}, SearchEngine};

#[cfg(all(windows, feature = "experimental-encrypted-search"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("missing mode")?.into_string().map_err(|_| "invalid mode")?;
    let index = PathBuf::from(args.next().ok_or("missing index")?);
    let docs = PathBuf::from(args.next().ok_or("missing documents root")?);
    std::fs::create_dir_all(&docs)?;
    let file = docs.join("wincommander-paired-synthetic.txt");
    let config = IndexConfig { roots: vec![docs.clone()], exclusions: vec![], skip_paths: vec![], max_file_bytes: 1024 * 1024, index_dir: index.clone() };
    let engine = match mode.as_str() {
        "seed" => {
            std::fs::write(&file, "syntheticlegacyquartz42 protected search fixture")?;
            let engine = SearchEngine::open(config)?;
            engine.start_indexing()?;
            engine
        }
        "verify" => {
            let selection = index.parent().ok_or("missing index parent")?.join(".wincommander-encrypted-selection");
            SearchEngine::open_experimental_selected_with_policy(config, Arc::new(|_| true), &selection)?
        }
        _ => return Err("unknown mode".into()),
    };
    let query = ContentQuery { terms: "syntheticlegacyquartz42".into(), roots: vec![docs], limit: 20, offset: 0, keyword_only: true };
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if engine.search(&query)?.iter().any(|hit| hit.path.eq_ignore_ascii_case(&file.to_string_lossy())) { break; }
        if Instant::now() >= deadline { return Err("synthetic document was not searchable".into()); }
        std::thread::sleep(Duration::from_millis(500));
    }
    engine.stop();
    println!("Synthetic document is searchable in {mode} index.");
    Ok(())
}

#[cfg(not(all(windows, feature = "experimental-encrypted-search")))]
fn main() { panic!("paired installed fixture requires Windows and experimental search"); }
