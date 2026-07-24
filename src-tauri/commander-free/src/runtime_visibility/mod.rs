// Runtime Visibility Manager — see scanner/enumerate/state/registry/actions
// for per-file responsibilities. Submodules are `pub` so the Tauri
// `generate_handler!` macro in lib.rs can resolve the `__cmd__*` helpers
// that `#[tauri::command]` generates alongside each command function.

pub mod actions;
pub mod enumerate;
pub mod registry;
pub mod scanner;
pub mod state;
