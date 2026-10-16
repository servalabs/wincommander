// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

struct DisposableKey {
    key: Option<Key>,
    path: String,
}
impl Drop for DisposableKey {
    fn drop(&mut self) {
        drop(self.key.take());
        // Only this newly-created, nonrecursive HKCU fixture is removed.
        unsafe {
            RegDeleteKeyExW(
                HKEY_CURRENT_USER,
                wide(&self.path).as_ptr(),
                KEY_WOW64_64KEY,
                0,
            );
        }
    }
}

fn fixture(descriptor: &Descriptor) -> DisposableKey {
    static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = format!(
        "SOFTWARE\\WinCommanderBluetoothControlTest-{}-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut key = std::ptr::null_mut();
    let mut disposition = 0;
    assert_eq!(
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                &descriptor.attributes(),
                &mut key,
                &mut disposition,
            )
        },
        ERROR_SUCCESS
    );
    assert_eq!(disposition, REG_CREATED_NEW_KEY);
    DisposableKey {
        key: Some(Key(key)),
        path,
    }
}

#[test]
fn native_disposable_registry_fixture_verifies_acl_and_durable_value_without_machine_state() {
    if super::super::windows::require_admin().is_err() {
        eprintln!("Native protected-registry acceptance requires an elevated test process.");
        return;
    }
    let fixture = fixture(&Descriptor::new().unwrap());
    let key = fixture.key.as_ref().unwrap();
    security::verify(key.0.cast(), SE_REGISTRY_KEY, 0x500d_0006).unwrap();
    let bytes = serde_json::to_vec(&Journal {
        version: 1,
        blocked: true,
        records: vec![],
    })
    .unwrap();
    assert_eq!(
        unsafe {
            RegSetValueExW(
                key.0,
                wide(VALUE).as_ptr(),
                0,
                REG_BINARY,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        },
        ERROR_SUCCESS
    );
    assert_eq!(unsafe { RegFlushKey(key.0) }, ERROR_SUCCESS);
    let mut actual = vec![0u8; 128];
    let mut count = actual.len() as u32;
    let mut kind = 0;
    assert_eq!(
        unsafe {
            RegQueryValueExW(
                key.0,
                wide(VALUE).as_ptr(),
                std::ptr::null(),
                &mut kind,
                actual.as_mut_ptr(),
                &mut count,
            )
        },
        ERROR_SUCCESS
    );
    assert_eq!(kind, REG_BINARY);
    assert_eq!(&actual[..count as usize], bytes.as_slice());
}

#[test]
fn native_registry_fixture_rejects_user_writable_recovery_authority() {
    if super::super::windows::require_admin().is_err() {
        return;
    }
    let sddl = wide("O:BAG:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)(A;;KA;;;BU)");
    let mut pointer = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl.as_ptr(), 1, &mut pointer, std::ptr::null_mut())
        },
        0
    );
    let descriptor = Descriptor(pointer);
    let fixture = fixture(&descriptor);
    assert!(security::verify(
        fixture.key.as_ref().unwrap().0.cast(),
        SE_REGISTRY_KEY,
        0x500d_0006
    )
    .is_err());
}
