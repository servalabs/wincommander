// SPDX-License-Identifier: AGPL-3.0-or-later
//! Open the Windows calculator without changing the locked session.

use tauri::Manager;

#[cfg(windows)]
fn activate_windows_calculator() -> Result<(), String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        ApplicationActivationManager, IApplicationActivationManager, AO_NONE,
    };

    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let initialized =
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
        return Err("Windows Calculator could not be opened.".into());
    }
    let _apartment = Apartment(initialized.is_ok());
    let manager: IApplicationActivationManager =
        unsafe { CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|_| "Windows Calculator could not be opened.".to_string())?;
    let process_id = unsafe {
        manager.ActivateApplication(
            w!("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            PCWSTR::null(),
            AO_NONE,
        )
    }
    .map_err(|_| "Windows Calculator could not be opened.".to_string())?;
    if process_id == 0 {
        return Err("Windows Calculator could not be opened.".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn activate_windows_calculator() -> Result<(), String> {
    Err("Windows Calculator is only available on Windows.".into())
}

fn finish_handoff(
    activation: Result<(), String>,
    still_locked: impl FnOnce() -> bool,
    hide: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    activation?;
    if still_locked() {
        hide()?;
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn open_windows_calculator(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" || !crate::calc_mode_active(window.app_handle()) {
        return Err("The calculator is not locked.".into());
    }
    let activation = tauri::async_runtime::spawn_blocking(activate_windows_calculator);
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), activation)
        .await
        .map_err(|_| "Windows Calculator took too long to open.".to_string())?
        .map_err(|_| "Windows Calculator could not be opened.".to_string())?;
    finish_handoff(
        result,
        || crate::calc_mode_active(window.app_handle()),
        || {
            window
                .hide()
                .map_err(|_| "The calculator could not be hidden.".to_string())?;
            let _ = window.set_skip_taskbar(true);
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::finish_handoff;
    use std::cell::Cell;

    #[test]
    fn failed_activation_keeps_the_locked_window_visible() {
        let hidden = Cell::new(false);
        let result = finish_handoff(
            Err("unavailable".into()),
            || true,
            || {
                hidden.set(true);
                Ok(())
            },
        );
        assert_eq!(result, Err("unavailable".into()));
        assert!(!hidden.get());
    }

    #[test]
    fn successful_activation_hides_only_a_still_locked_window() {
        for locked in [false, true] {
            let hidden = Cell::new(false);
            finish_handoff(
                Ok(()),
                || locked,
                || {
                    hidden.set(true);
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(hidden.get(), locked);
        }
    }
}
