// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::{
    inspect_path,
    native::{wide, Handle},
    private_slot, VolumeInfo,
};
use std::{marker::PhantomData, rc::Rc};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, SECURITY_ATTRIBUTES},
    System::Threading::*,
};

const WAIT_MILLIS: u32 = 5_000;
const MUTEX_ACCESS: u32 = SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE;

/// Cross-process coordination only; authorization remains with the filesystem and service.
/// The owning OS thread must also release the mutex, so this guard is not Send or Sync.
pub struct VolumeOperationGuard {
    handle: Handle,
    _thread_bound: PhantomData<Rc<()>>,
}

impl VolumeOperationGuard {
    /// Acquire the native slot and revalidate the snapshot after any wait.
    pub fn acquire(info: &VolumeInfo) -> Result<Self, String> {
        if !info.is_private {
            return Err("Volume is not private".into());
        }
        let guard = Self::acquire_slot(private_slot(&info.device)?)?;
        let current = inspect_path(&info.root)?;
        if current != *info {
            return Err("Private volume changed while waiting for access".into());
        }
        Ok(guard)
    }

    /// Service-owned dismounts use a slot even when its presentation is already removed.
    pub fn acquire_slot(slot: u8) -> Result<Self, String> {
        if slot > 25 {
            return Err("Invalid VeraCrypt device slot".into());
        }
        Self::acquire_named(
            &format!(r"Global\WinCommander.VeraCrypt.Operation.v1.{slot}"),
            WAIT_MILLIS,
        )
    }

    fn acquire_named(name: &str, timeout: u32) -> Result<Self, String> {
        // Authenticated users may wait/release but cannot rewrite the service's DACL.
        let sddl = wide("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x00100001;;;AU)")?;
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
            return Err("Volume coordination security unavailable".into());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let name = wide(name)?;
        let raw = unsafe { CreateMutexExW(&attributes, name.as_ptr(), 0, MUTEX_ACCESS) };
        unsafe {
            LocalFree(descriptor);
        }
        let handle = Handle::new(raw)?;
        match unsafe { WaitForSingleObject(handle.0, timeout) } {
            WAIT_OBJECT_0 => Ok(Self {
                handle,
                _thread_bound: PhantomData,
            }),
            WAIT_ABANDONED => {
                unsafe {
                    ReleaseMutex(handle.0);
                }
                Err("Previous volume operation ended unexpectedly; retry after reinspection".into())
            }
            WAIT_TIMEOUT => Err("Private volume is busy; retry the operation".into()),
            _ => Err("Volume coordination unavailable".into()),
        }
    }
}

impl Drop for VolumeOperationGuard {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.handle.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn operation_wait_is_bounded_and_release_allows_next_thread() {
        let name = format!(r"Local\WinCommander.VolumeTest.{}", std::process::id());
        let held = VolumeOperationGuard::acquire_named(&name, 100).unwrap();
        let other_name = name.clone();
        let blocked = std::thread::spawn(move || {
            VolumeOperationGuard::acquire_named(&other_name, 20).is_err()
        });
        assert!(blocked.join().unwrap());
        drop(held);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let next = VolumeOperationGuard::acquire_named(&name, 100);
            sender.send(next.is_ok()).unwrap();
        })
        .join()
        .unwrap();
        assert!(receiver.recv().unwrap());
    }

    #[test]
    fn rejects_out_of_range_slot_without_creating_a_mutex() {
        assert!(VolumeOperationGuard::acquire_slot(26).is_err());
    }

    #[test]
    fn coordinates_with_a_separate_process() {
        let name = format!(
            r"Local\WinCommander.ProcessVolumeTest.{}",
            std::process::id()
        );
        let probe = |expected: bool| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "operation::tests::child_lock_probe"])
                .env("WINCMD_VOLUME_TEST_MUTEX", &name)
                .env("WINCMD_VOLUME_TEST_EXPECTED", expected.to_string())
                .output()
                .unwrap()
                .status
                .success()
        };
        let held = VolumeOperationGuard::acquire_named(&name, 100).unwrap();
        assert!(
            probe(false),
            "second process must time out while mutex is held"
        );
        drop(held);
        assert!(probe(true), "second process must acquire after release");
    }

    #[test]
    fn child_lock_probe() {
        let Ok(name) = std::env::var("WINCMD_VOLUME_TEST_MUTEX") else {
            return;
        };
        let expected = std::env::var("WINCMD_VOLUME_TEST_EXPECTED").unwrap() == "true";
        assert_eq!(
            VolumeOperationGuard::acquire_named(&name, 50).is_ok(),
            expected
        );
    }
}
