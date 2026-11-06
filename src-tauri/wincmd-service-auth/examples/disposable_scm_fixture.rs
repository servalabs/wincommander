// SPDX-License-Identifier: AGPL-3.0-or-later
// A dedicated, temporary SCM service for CI only. Never used by the product.
#[cfg(windows)]
mod windows_fixture {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
    };

    const NAME: &str = "WinCommanderSearchTestFixture";
    define_windows_service!(ffi_service_main, service_main);
    fn service_main(_: Vec<OsString>) {
        let (tx, rx) = mpsc::channel();
        let handle = service_control_handler::register(NAME, move |control| match control {
            ServiceControl::Stop => {
                let _ = tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })
        .unwrap();
        let status = |state, controls| ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: controls,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::ZERO,
            process_id: None,
        };
        handle
            .set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP))
            .unwrap();
        let _ = rx.recv();
        handle
            .set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))
            .unwrap();
    }
    pub fn main() {
        service_dispatcher::start(NAME, ffi_service_main).unwrap();
    }
}
#[cfg(windows)]
fn main() {
    windows_fixture::main();
}
#[cfg(not(windows))]
fn main() {
    panic!("Windows fixture only");
}
