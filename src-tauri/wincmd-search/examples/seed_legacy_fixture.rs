// Disposable CI-only legacy cache seeder. Never used by the application.
use std::path::PathBuf;
use wincmd_search::{SearchEngine, types::IndexConfig};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("missing disposable path")?);
    std::fs::create_dir_all(&path)?;
    let engine = SearchEngine::open(IndexConfig { roots: vec![], exclusions: vec![], skip_paths: vec![], max_file_bytes: 1024 * 1024, index_dir: path })?;
    engine.stop();
    Ok(())
}
