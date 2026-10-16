// SPDX-License-Identifier: AGPL-3.0-or-later
//! Fixed SYSTEM WMI operations; OS eligibility always requires a live provider check.
use super::{
    engine::{Action, Journal, Observation, Platform as PolicyPlatform, MANAGED, UNVERIFIED},
    journal,
};
use windows_sys::Win32::System::SystemInformation::{GetProductInfo, OSVERSIONINFOEXW};
#[path = "management.rs"]
mod management;
#[path = "provider_process.rs"]
mod process;
#[path = "provider_script.rs"]
mod script;

fn request(action: &str, expected: &str, receipt: &str) -> Result<Observation, String> {
    let registered = management::registered()?;
    let flag = if registered { "$true" } else { "$false" };
    let observation = process::execute(&format!(
        "$action='{action}';$expected='{expected}';$receipt='{receipt}';$mdmRegistered={flag}\n{}",
        script::SCRIPT
    ))?;
    if management::registered()? != registered {
        return Err(MANAGED.into());
    }
    Ok(observation)
}

#[derive(Default)]
pub(super) struct Platform {
    context: Option<String>,
}
pub(super) fn require_admin() -> Result<(), String> {
    if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0 {
        return Err("[BT_ACCESS_DENIED] Administrator approval is required to change Bluetooth policy for this PC.".into());
    }
    Ok(())
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(info: *mut OSVERSIONINFOEXW) -> i32;
}

pub(super) fn eligible_os(product_type: u8, build: u32, sku: u32) -> bool {
    if matches!(product_type, 2 | 3) {
        return build >= 26100 && !matches!(sku, 0 | u32::MAX);
    }
    product_type == 1
        && build >= 10240
        && matches!(
            sku,
            4 | 27
                | 48
                | 49
                | 69
                | 70
                | 72
                | 84
                | 103
                | 121
                | 122
                | 125
                | 126
                | 129
                | 130
                | 161
                | 162
                | 188
                | 191
        )
}

fn eligible_current_os() -> Result<bool, String> {
    let mut info: OSVERSIONINFOEXW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOEXW>() as u32;
    if unsafe { RtlGetVersion(&mut info) } != 0 {
        return Err(UNVERIFIED.into());
    }
    let mut sku = 0;
    if unsafe {
        GetProductInfo(
            info.dwMajorVersion,
            info.dwMinorVersion,
            info.wServicePackMajor as u32,
            info.wServicePackMinor as u32,
            &mut sku,
        )
    } == 0
    {
        return Err(UNVERIFIED.into());
    }
    Ok(info.dwMajorVersion >= 10 && eligible_os(info.wProductType, info.dwBuildNumber, sku))
}

fn unsupported() -> Observation {
    Observation {
        supported: false,
        managed: false,
        context: String::new(),
        configured: None,
        effective: None,
    }
}

impl PolicyPlatform for Platform {
    fn admin(&self) -> bool {
        require_admin().is_ok()
    }
    fn legacy_active(&mut self) -> Result<bool, String> {
        match journal::read_bytes("SOFTWARE\\WinCommanderBluetoothControlV1")? {
            Some(bytes) => super::legacy::active(&bytes),
            None => Ok(false),
        }
    }
    fn read(&mut self) -> Result<Journal, String> {
        journal::read()
    }
    fn write(&mut self, value: &Journal) -> Result<(), String> {
        journal::write(value)
    }
    fn observe(&mut self) -> Result<Observation, String> {
        if !eligible_current_os()? {
            return Ok(unsupported());
        }
        let value = match request("read", "none", "") {
            Ok(value) => value,
            Err(error) if error == super::engine::UNSUPPORTED => return Ok(unsupported()),
            Err(error) => return Err(error),
        };
        self.context = Some(value.context.clone());
        Ok(value)
    }
    fn apply(&mut self, action: Action) -> Result<Observation, String> {
        require_admin()?;
        if !eligible_current_os()? {
            return Err(super::engine::UNSUPPORTED.into());
        }
        let context = self.context.take().ok_or(UNVERIFIED)?;
        if context.len() != 64 || !context.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(UNVERIFIED.into());
        }
        let (action, expected) = match action {
            Action::Block { expected: None } => ("block", "none"),
            Action::Block { expected: Some(2) } => ("block", "2"),
            Action::Allow => ("allow", "0"),
            _ => return Err(UNVERIFIED.into()),
        };
        request(action, expected, &context)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn server_2025_can_probe_policy_but_home_old_and_unknown_os_cannot() {
        for product_type in [2, 3] {
            for sku in [7, 8] {
                assert!(super::eligible_os(product_type, 26100, sku));
                assert!(!super::eligible_os(product_type, 20348, sku));
            }
            assert!(!super::eligible_os(product_type, 26100, 0));
            assert!(!super::eligible_os(product_type, 26100, u32::MAX));
        }
        assert!(!super::eligible_os(0, 26100, 7));
        assert!(!super::eligible_os(1, 26100, 101));
        assert!(!super::eligible_os(1, 9600, 48));
        assert!(!super::eligible_os(1, 26100, u32::MAX));
        for sku in [48, 49, 4, 121, 122, 188, 191] {
            assert!(super::eligible_os(1, 26100, sku));
        }
    }
    #[test]
    fn real_os_eligibility_probe_does_not_change_policy() {
        let eligible = super::eligible_current_os().expect("Windows OS eligibility observation");
        eprintln!("Read-only native Bluetooth policy OS eligibility: {eligible}");
    }
}
