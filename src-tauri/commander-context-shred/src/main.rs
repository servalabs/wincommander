// Explorer starts this helper directly. Keeping the Windows subsystem avoids a
// console flash while the embedded asInvoker manifest keeps the caller's token.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut arguments = std::env::args().skip(1);
    // The registry verb uses this explicit marker so the helper cannot be
    // confused with an arbitrary executable launch. It is an action flag, not
    // an Explorer path, and must never reach the strict path validator.
    if arguments.next().as_deref() != Some("--context-shred") {
        std::process::exit(1);
    }
    let paths = arguments.collect();
    let exit_code = match commander_context_shred::execute_cli(paths) {
        Ok(()) => 0,
        Err(error) => {
            commander_context_shred::log_result(&error);
            1
        }
    };
    std::process::exit(exit_code);
}
