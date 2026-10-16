// SPDX-License-Identifier: AGPL-3.0-or-later
//! ACLs and cross-process serialization are load-bearing, not best-effort repair.
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{
        self,
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
            SE_KERNEL_OBJECT, SE_OBJECT_TYPE,
        },
        CreateWellKnownSid, EqualSid, GetAce, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        SECURITY_ATTRIBUTES,
    },
    System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
};

pub(super) const ACL_ERROR: &str = "[BT_STATE_UNVERIFIED] Bluetooth machine-state protection could not be verified. No adapter will be changed; contact your administrator.";

pub(super) struct Descriptor(pub Security::PSECURITY_DESCRIPTOR);
impl Descriptor {
    pub fn new() -> Result<Self, String> {
        // Explicit trusted owner, protected DACL. Users may observe status only.
        let sddl: Vec<u16> = "O:BAG:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KR;;;BU)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(ACL_ERROR.into());
        }
        Ok(Self(descriptor))
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

pub(super) fn verify(handle: HANDLE, kind: SE_OBJECT_TYPE, write_mask: u32) -> Result<(), String> {
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        if GetSecurityInfo(
            handle,
            kind,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        ) != 0
        {
            return Err(ACL_ERROR.into());
        }
        let _descriptor = Descriptor(descriptor);
        let mut system = [0u32; 17];
        let mut admins = [0u32; 17];
        let mut bytes = 68;
        if CreateWellKnownSid(
            WinLocalSystemSid,
            std::ptr::null_mut(),
            system.as_mut_ptr().cast(),
            &mut bytes,
        ) == 0
        {
            return Err(ACL_ERROR.into());
        }
        bytes = 68;
        if CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            std::ptr::null_mut(),
            admins.as_mut_ptr().cast(),
            &mut bytes,
        ) == 0
        {
            return Err(ACL_ERROR.into());
        }
        let trusted = |sid| {
            EqualSid(sid, system.as_ptr().cast_mut().cast()) != 0
                || EqualSid(sid, admins.as_ptr().cast_mut().cast()) != 0
        };
        if owner.is_null() || !trusted(owner) || acl.is_null() || (*acl).AceCount == 0 {
            return Err(ACL_ERROR.into());
        }
        let mut system_write = false;
        let mut admin_write = false;
        for index in 0..(*acl).AceCount {
            let mut ace = std::ptr::null_mut();
            if GetAce(acl, index as u32, &mut ace) == 0 {
                return Err(ACL_ERROR.into());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            // Unsupported/callback/object/deny ACLs are uncertain; do not reinterpret.
            if ace.Header.AceType != 0 {
                return Err(ACL_ERROR.into());
            }
            let sid = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
            let writes = ace.Mask & write_mask != 0;
            if writes && !trusted(sid) {
                return Err(ACL_ERROR.into());
            }
            if writes && EqualSid(sid, system.as_ptr().cast_mut().cast()) != 0 {
                system_write = true;
            }
            if writes && EqualSid(sid, admins.as_ptr().cast_mut().cast()) != 0 {
                admin_write = true;
            }
        }
        if !system_write || !admin_write {
            return Err(ACL_ERROR.into());
        }
    }
    Ok(())
}

pub(super) struct MachineLock(HANDLE);
impl MachineLock {
    pub fn acquire() -> Result<Self, String> {
        // Mutex permissions use kernel-object rights rather than registry rights.
        let sddl: Vec<u16> = "O:BAG:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(ACL_ERROR.into());
        }
        let descriptor = Descriptor(descriptor);
        let name: Vec<u16> = "Global\\WinCommander_BluetoothPolicy_v2\0"
            .encode_utf16()
            .collect();
        let handle = unsafe { CreateMutexW(&descriptor.attributes(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(
                "[BT_STATE_UNVERIFIED] Bluetooth operation lock is unavailable. Retry shortly."
                    .into(),
            );
        }
        if let Err(error) = verify(handle, SE_KERNEL_OBJECT, 0x001f0001 | 0x10000000) {
            unsafe {
                CloseHandle(handle);
            }
            return Err(error);
        }
        match unsafe { WaitForSingleObject(handle, 5000) } {
            0 | 0x80 => Ok(Self(handle)),
            _ => {
                unsafe {
                    CloseHandle(handle);
                }
                Err(
                    "[BT_OPERATION_BUSY] Another Bluetooth operation is busy. Retry shortly."
                        .into(),
                )
            }
        }
    }
}
impl Drop for MachineLock {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0);
            CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mutex_serializes_workers_without_touching_devices_or_registry() {
        if super::super::provider::require_admin().is_err() {
            eprintln!("Native protected-mutex acceptance requires an elevated test process.");
            return;
        }
        let guard = MachineLock::acquire().expect("protected global mutex must be creatable");
        let entered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_entered = entered.clone();
        let worker = std::thread::spawn(move || {
            let _guard = MachineLock::acquire().expect("second worker must acquire after release");
            worker_entered.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!entered.load(std::sync::atomic::Ordering::SeqCst));
        drop(guard);
        worker.join().unwrap();
        assert!(entered.load(std::sync::atomic::Ordering::SeqCst));
    }
}
