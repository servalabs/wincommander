// CI-only disposable SCM service; never used by a product binary.
#[cfg(windows)]
mod fixture {
    use std::{ffi::OsString, sync::mpsc, time::Duration};
    use windows_service::{define_windows_service, service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType}, service_control_handler::{self, ServiceControlHandlerResult}, service_dispatcher};
    define_windows_service!(ffi_main, service_main);
    fn service_main(args: Vec<OsString>) {
        let Some(name) = args.first() else { return; };
        let (tx, rx) = mpsc::channel();
        let handle = service_control_handler::register(name, move |control| match control {
            ServiceControl::Stop => { let _ = tx.send(()); ServiceControlHandlerResult::NoError },
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }).unwrap();
        let status = |state, controls| ServiceStatus { service_type: ServiceType::OWN_PROCESS,
            current_state: state, controls_accepted: controls, exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0, wait_hint: Duration::ZERO, process_id: None };
        handle.set_service_status(status(ServiceState::Running, ServiceControlAccept::STOP)).unwrap();
        let _ = rx.recv();
        handle.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty())).unwrap();
    }
    pub fn main() {
        let name = std::env::args_os().nth(1).expect("missing disposable service name");
        service_dispatcher::start(name, ffi_main).unwrap();
    }
}
#[cfg(windows)] fn main() { fixture::main(); }
#[cfg(not(windows))] fn main() { panic!("Windows fixture only"); }
