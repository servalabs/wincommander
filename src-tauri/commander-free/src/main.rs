// Prevents a console window on Windows, DO NOT REMOVE!! Unconditional (not
// gated on `not(debug_assertions)`): a `--safe-copy`/`--safe-paste`/`--scrub`
// context-menu launch must never flash a console regardless of which profile
// built the running binary — the release pipeline already disables
// debug_assertions (no `[profile.release] debug-assertions` override in
// Cargo.toml, CI's release.yml runs `cargo build --release`), but this stays
// defense-in-depth against a stray non-release build ever being shipped.
// `cargo run`/`tauri dev` lose the debug console as a result; panics and app
// logs already go to the unified log file (see the panic hook in lib.rs::run),
// so this isn't a debugging regression in practice.
#![windows_subsystem = "windows"]

fn main() {
    wincommander_lib::run()
}
