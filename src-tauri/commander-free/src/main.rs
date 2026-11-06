// Every GUI invocation, including a local/debug Explorer context-menu launch,
// must be windowless. CLI mode below explicitly attaches to the caller's
// terminal, so diagnostics remain available without a PowerShell/WinCommander
// console flashing when someone securely deletes a file.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("--search-migration-transaction") {
        #[cfg(feature = "experimental-encrypted-search")]
        {
            let enabled = std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SEARCH_MIGRATION").as_deref() == Some(std::ffi::OsStr::new("1"));
            let result = if enabled && args.len() == 3 {
                wincommander_lib::search_migration_cli::run_transaction(&args[1], &args[2])
            } else { Err("Search migration transaction is disabled or invalid.".into()) };
            match result {
                Ok(outcome) => println!("WINCOMMANDER_SEARCH_TRANSACTION_OUTCOME:{outcome}"),
                Err(error) => { eprintln!("{error}"); std::process::exit(1); },
            }
            return;
        }
        #[cfg(not(feature = "experimental-encrypted-search"))]
        std::process::exit(1);
    }
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("--search-migration-lab") {
        #[cfg(feature = "experimental-encrypted-search")]
        {
            let enabled = std::env::var_os("WINCOMMANDER_EXPERIMENTAL_SEARCH_MIGRATION").as_deref() == Some(std::ffi::OsStr::new("1"));
            let result = if enabled && args.len() == 2 {
                wincommander_lib::search_migration_cli::run(&args[1])
            } else { Err("Search migration lab is disabled or its arguments are invalid.".into()) };
            match result {
                Ok(outcome) => println!("WINCOMMANDER_SEARCH_MIGRATION_OUTCOME:{outcome}"),
                Err(error) => { eprintln!("{error}"); std::process::exit(1); },
            }
            return;
        }
        #[cfg(not(feature = "experimental-encrypted-search"))]
        std::process::exit(1);
    }
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("--hold-search-maintenance-fence") {
        #[cfg(feature = "experimental-settings-generation")]
        {
            let result = args.get(1).and_then(|value| value.parse::<u32>().ok())
                .filter(|_| args.len() == 2)
                .ok_or_else(|| "Invalid installer process.".to_string())
                .and_then(wincommander_lib::settings_generation_lab::hold_search_maintenance_fence);
            if let Err(error) = result { eprintln!("{error}"); std::process::exit(1); }
            return;
        }
        #[cfg(not(feature = "experimental-settings-generation"))]
        std::process::exit(1);
    }
    #[cfg(windows)]
    if args.first().map(String::as_str) == Some("--settings-generation-lab") {
        attach_parent_console();
        #[cfg(feature = "experimental-settings-generation")]
        {
            let outcome = wincommander_lib::settings_generation_lab::run(&args)
                .expect("matched settings generation lab command");
            match outcome {
                Ok(message) => { println!("{message}"); std::process::exit(0); }
                Err(message) => {
                    let code = if message.starts_with("SETTINGS_IDENTITY_UNVERIFIED:") { 2 } else { 1 };
                    eprintln!("{message}"); std::process::exit(code);
                }
            }
        }
        #[cfg(not(feature = "experimental-settings-generation"))]
        {
            eprintln!("Settings migration is not included in this build; existing data was preserved.");
            std::process::exit(1);
        }
    }
    if args.as_slice() == ["--hide-recovery-listener"] {
        wincommander_lib::run_hide_recovery_listener();
        return;
    }
    if args.len() == 2 && args[0] == "--hide-recovery-update" {
        std::process::exit(wincommander_lib::update_hide_recovery_listener(&args[1]));
    }
    // A limited desktop session asks Windows for this one narrow UAC helper
    // when it needs to replace the machine-wide Pro sidecar.  Handle it before
    // CLI/Tauri startup so the elevated child never creates a second window,
    // listener, or normal application instance.
    if let Some(exit_code) = wincommander_lib::run_machine_pro_update_if_requested(&args) {
        std::process::exit(exit_code);
    }
    // A true NSIS uninstall releases the device seat before removing the
    // executable. `/UPDATE` never passes this argument, so updates retain the
    // existing token and seat without asking the user to activate again.
    if let Some(exit_code) = wincommander_lib::run_license_seat_release_if_requested(&args) {
        std::process::exit(exit_code);
    }
    #[cfg(all(feature = "autonomous-test", debug_assertions))]
    if wincommander_lib::autonomous_test::is_invocation(&args) {
        attach_parent_console();
        std::process::exit(wincommander_lib::autonomous_test::main(args));
    }
    #[cfg(all(feature = "autonomous-test", debug_assertions))]
    if wincommander_lib::autonomous_agent_test::is_invocation(&args) {
        attach_parent_console();
        std::process::exit(wincommander_lib::autonomous_agent_test::main(args));
    }
    if wincommander_lib::cli::is_cli_invocation(&args) {
        attach_parent_console();
        std::process::exit(wincommander_lib::cli::main(args));
    }
    wincommander_lib::run();
}

#[cfg(windows)]
fn attach_parent_console() {
    // The binary remains a Windows-subsystem application so Explorer and
    // context-menu launches never flash a console. CLI mode explicitly joins
    // the caller's terminal, allowing the same shipped executable to emit JSON.
    unsafe {
        let _ = windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );

        use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows_sys::Win32::System::Console::{
            GetStdHandle, SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };

        let existing_output = GetStdHandle(STD_OUTPUT_HANDLE);
        if existing_output.is_null() || existing_output == INVALID_HANDLE_VALUE {
            let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let output = CreateFileW(
                conout.as_ptr(),
                GENERIC_WRITE | GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            );
            if output != INVALID_HANDLE_VALUE {
                let _ = SetStdHandle(STD_OUTPUT_HANDLE, output);
                let _ = SetStdHandle(STD_ERROR_HANDLE, output);
            }
        }

        let existing_input = GetStdHandle(STD_INPUT_HANDLE);
        if existing_input.is_null() || existing_input == INVALID_HANDLE_VALUE {
            let conin: Vec<u16> = "CONIN$\0".encode_utf16().collect();
            let input = CreateFileW(
                conin.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            );
            if input != INVALID_HANDLE_VALUE {
                let _ = SetStdHandle(STD_INPUT_HANDLE, input);
            }
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}
