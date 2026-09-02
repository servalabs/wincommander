// Every GUI invocation, including a local/debug Explorer context-menu launch,
// must be windowless. CLI mode below explicitly attaches to the caller's
// terminal, so diagnostics remain available without a PowerShell/WinCommander
// console flashing when someone securely deletes a file.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(all(feature = "autonomous-test", debug_assertions))]
    if wincommander_lib::autonomous_test::is_invocation(&args) {
        attach_parent_console();
        std::process::exit(wincommander_lib::autonomous_test::main(args));
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
