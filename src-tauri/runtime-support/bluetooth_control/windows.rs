// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{
    engine::{Adapter, Journal, Platform as DevicePlatform},
    journal,
};
#[path = "errors.rs"]
mod errors;
use windows_sys::Win32::{
    Devices::{Bluetooth::GUID_BTHPORT_DEVICE_INTERFACE, DeviceAndDriverInstallation::*},
    Foundation::{GetLastError, ERROR_NO_MORE_ITEMS, INVALID_HANDLE_VALUE},
};

pub(super) struct Platform;

pub(super) fn require_admin() -> Result<(), String> {
    if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } == 0 {
        return Err(
            "[BT_ADMIN_REQUIRED] Administrator access is required to change Bluetooth for this PC."
                .into(),
        );
    }
    Ok(())
}

impl DevicePlatform for Platform {
    fn admin(&self) -> bool {
        require_admin().is_ok()
    }
    fn read(&mut self) -> Result<Journal, String> {
        journal::read()
    }
    fn write(&mut self, value: &Journal) -> Result<(), String> {
        journal::write(value)
    }
    fn adapters(&mut self) -> Result<Vec<Adapter>, String> {
        enumerate()
    }
    fn externally_managed(&mut self) -> Result<bool, String> {
        journal::external_policy()
    }
    fn disable(&mut self, adapter: &Adapter) -> Result<(), String> {
        change(adapter, false)
    }
    fn enable(&mut self, adapter: &Adapter) -> Result<(), String> {
        // Recheck policy immediately before each enable, not only at batch start.
        if journal::external_policy()? {
            return Err("Another device policy prevents Bluetooth restoration.".into());
        }
        change(adapter, true)
    }
}

struct DeviceSet(HDEVINFO);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

fn registry_property(
    set: HDEVINFO,
    data: &SP_DEVINFO_DATA,
    property: u32,
) -> Result<String, String> {
    let mut buffer = vec![0u16; 4096];
    let mut kind = 0;
    let mut required = 0;
    if unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            data as *const _ as *mut _,
            property,
            &mut kind,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 2) as u32,
            &mut required,
        )
    } == 0
        || required == 0
        || required > (buffer.len() * 2) as u32
        || required % 2 != 0
        || !matches!(kind, 1 | 7)
    {
        return Err("Windows could not verify a Bluetooth adapter identity. No uncertain adapter will be changed.".into());
    }
    buffer.truncate(required as usize / 2);
    Ok(String::from_utf16_lossy(&buffer)
        .trim_end_matches('\0')
        .to_uppercase())
}

fn device(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> Result<Option<Adapter>, String> {
    // BTHPORT interface is a radio-specific interface. Require Bluetooth setup class
    // too, excluding Bluetooth children and a combined Wi-Fi/USB parent device.
    const BLUETOOTH_CLASS: windows_sys::core::GUID =
        windows_sys::core::GUID::from_u128(0xe0cbf06c_cd8b_4647_bb8a_263b43f0f974);
    let class = data.ClassGuid;
    if class.data1 != BLUETOOTH_CLASS.data1
        || class.data2 != BLUETOOTH_CLASS.data2
        || class.data3 != BLUETOOTH_CLASS.data3
        || class.data4 != BLUETOOTH_CLASS.data4
    {
        return Ok(None);
    }
    let mut flags = 0;
    let mut problem = 0;
    let result = unsafe { CM_Get_DevNode_Status(&mut flags, &mut problem, data.DevInst, 0) };
    if result == CR_NO_SUCH_DEVNODE {
        return Ok(None);
    }
    if result != CR_SUCCESS {
        return Err("Windows could not read Bluetooth adapter state. Refresh and retry.".into());
    }
    // A non-present phantom does not have a started or problematic devnode.
    if flags & DN_STARTED == 0 && flags & DN_HAS_PROBLEM == 0 {
        return Ok(None);
    }
    let mut id = vec![0u16; 1024];
    if unsafe { CM_Get_Device_IDW(data.DevInst, id.as_mut_ptr(), id.len() as u32, 0) } != CR_SUCCESS
    {
        return Err("Windows could not identify a Bluetooth adapter. Refresh and retry.".into());
    }
    let end = id
        .iter()
        .position(|v| *v == 0)
        .ok_or("Bluetooth adapter identity is too long.")?;
    let id = String::from_utf16_lossy(&id[..end]).to_uppercase();
    let mut present_id: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    let mut present_node = 0;
    let present = unsafe {
        CM_Locate_DevNodeW(
            &mut present_node,
            present_id.as_mut_ptr(),
            CM_LOCATE_DEVNODE_NORMAL,
        )
    };
    if present == CR_NO_SUCH_DEVNODE {
        return Ok(None);
    }
    if present != CR_SUCCESS || present_node != data.DevInst {
        return Err("Bluetooth adapter presence changed. Refresh and retry.".into());
    }
    let hardware = registry_property(set, data, SPDRP_HARDWAREID)?;
    let driver = registry_property(set, data, SPDRP_DRIVER)?;
    let service = registry_property(set, data, SPDRP_SERVICE)?;
    let mut container: windows_sys::core::GUID = unsafe { std::mem::zeroed() };
    let mut kind = 0;
    let mut bytes = 0;
    if unsafe {
        SetupDiGetDevicePropertyW(
            set,
            data,
            &windows_sys::Win32::Devices::Properties::DEVPKEY_Device_ContainerId,
            &mut kind,
            (&mut container as *mut windows_sys::core::GUID).cast(),
            16,
            &mut bytes,
            0,
        )
    } == 0
        || kind != windows_sys::Win32::Devices::Properties::DEVPROP_TYPE_GUID
        || bytes != 16
    {
        return Err("Windows could not verify the Bluetooth adapter container. No uncertain adapter will be changed.".into());
    }
    let container = format!(
        "{:08x}-{:04x}-{:04x}-{:02x?}",
        container.data1, container.data2, container.data3, container.data4
    );
    let identity = format!("{hardware}|{driver}|{service}|{container}");
    if id.is_empty() || id.len() > 1024 || identity.len() > 8192 {
        return Err("Bluetooth adapter identity exceeds the safe recovery limit. No adapter will be changed.".into());
    }
    Ok(Some(Adapter {
        id,
        identity,
        problem,
    }))
}

fn enumerate() -> Result<Vec<Adapter>, String> {
    // Do not request only active interfaces: disabled radios must remain observable
    // after reboot so only the recorded radio can be restored.
    let raw = unsafe {
        SetupDiGetClassDevsW(
            &GUID_BTHPORT_DEVICE_INTERFACE,
            std::ptr::null(),
            std::ptr::null_mut(),
            DIGCF_DEVICEINTERFACE,
        )
    };
    if raw == INVALID_HANDLE_VALUE as isize {
        return Err("Windows could not enumerate Bluetooth adapters. Refresh and retry.".into());
    }
    let set = DeviceSet(raw);
    let mut found = Vec::new();
    for index in 0..=128 {
        let mut data: SP_DEVINFO_DATA = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
        if unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut data) } == 0 {
            if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                return Ok(found);
            }
            return Err("Windows Bluetooth enumeration was interrupted. Refresh and retry.".into());
        }
        if index == 128 {
            return Err("Too many Bluetooth adapter records. Ask your administrator to review device history.".into());
        }
        if let Some(adapter) = device(set.0, &data)? {
            found.push(adapter);
        }
    }
    unreachable!()
}

fn change(expected: &Adapter, enabled: bool) -> Result<(), String> {
    require_admin()?;
    // Resolve afresh and compare the complete journalled identity at mutation time.
    let current = enumerate()?
        .into_iter()
        .find(|a| a.id.eq_ignore_ascii_case(&expected.id))
        .ok_or("Bluetooth adapter disconnected. Reconnect it and retry.")?;
    if current.identity != expected.identity || current.problem != expected.problem {
        return Err(
            "[BT_STATE_CHANGED] Bluetooth adapter state changed. Refresh before retrying.".into(),
        );
    }
    let mut id: Vec<u16> = current.id.encode_utf16().chain(Some(0)).collect();
    let mut devinst = 0;
    if unsafe { CM_Locate_DevNodeW(&mut devinst, id.as_mut_ptr(), CM_LOCATE_DEVNODE_NORMAL) }
        != CR_SUCCESS
    {
        return Err(
            "[BT_DEVICE_UNAVAILABLE] Bluetooth adapter is no longer available. Refresh and retry."
                .into(),
        );
    }
    let result = unsafe {
        if enabled {
            CM_Enable_DevNode(devinst, 0)
        } else {
            CM_Disable_DevNode(devinst, CM_DISABLE_UI_NOT_OK | CM_DISABLE_PERSIST)
        }
    };
    if result != CR_SUCCESS {
        return Err(errors::native_rejected(enabled, result));
    }
    // Bound the normal asynchronous PnP transition; never report a native return
    // alone as verified success. Failure leaves the recovery record in place.
    for _ in 0..10 {
        let mut flags = 0;
        let mut problem = 0;
        if unsafe { CM_Get_DevNode_Status(&mut flags, &mut problem, devinst, 0) } == CR_SUCCESS
            && if enabled {
                problem == 0 && flags & DN_STARTED != 0
            } else {
                problem == 22
            }
        {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(errors::transition_timeout(enabled))
}

#[cfg(test)]
mod tests {
    #[test]
    fn real_windows_adapter_observation_is_read_only() {
        let adapters = super::enumerate().expect("native Bluetooth enumeration must be observable");
        eprintln!(
            "Read-only Windows Bluetooth probe: {} verified radio(s)",
            adapters.len()
        );
        assert!(adapters
            .iter()
            .all(|adapter| !adapter.id.is_empty() && !adapter.identity.is_empty()));
    }

    #[test]
    fn real_windows_external_policy_observation_does_not_write_policy() {
        let _ = super::journal::external_policy()
            .expect("Windows policy observation must be available");
    }
}
