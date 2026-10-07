// SPDX-License-Identifier: AGPL-3.0-or-later
use super::liveness::interactive_logon_ended;
use super::*;
use std::cell::Cell;
use std::collections::HashSet;

#[test]
fn vanished_primary_alias_does_not_hide_linked_alias_from_inventory() {
    let primary = (7, 0);
    let related = [primary, (8, 0)];
    let expected = r"\Device\VeraCryptVolumeK";
    assert_eq!(
        related_links_absent(primary, &related, expected, |logon| {
            Ok((logon != primary).then(|| expected.to_owned()))
        }),
        Ok(false)
    );
    assert_eq!(
        related_links_absent(primary, &related, expected, |_| Ok(None)),
        Ok(true)
    );
    assert_eq!(
        related_links_absent(primary, &related, expected, |logon| {
            if logon == primary {
                Ok(None)
            } else {
                Err(())
            }
        }),
        Err(())
    );
    assert_eq!(
        related_links_absent(primary, &related, expected, |logon| {
            Ok((logon != primary).then(|| r"\Device\HarddiskVolume4".to_owned()))
        }),
        Ok(true)
    );
    assert_eq!(
        related_links_absent(primary, &related, expected, |_| {
            Ok(Some(r"\Device\HarddiskVolume4".to_owned()))
        }),
        Ok(false)
    );
}

#[test]
fn accepts_only_literal_logon_directory_names() {
    assert_eq!(parse_logon_name("00000000-1234abcd"), Some((0x1234abcd, 0)));
    assert_eq!(
        parse_logon_name("ffffffff-1234ABCD"),
        Some((0x1234abcd, -1))
    );
    for invalid in [
        "",
        "0-1234abcd",
        "00000000-1234abcd\\K:",
        "00000000-1234abcg",
        "00000000/1234abcd",
        "é0000000-1234abcd",
    ] {
        assert_eq!(parse_logon_name(invalid), None);
    }
    assert!(link_path((0x1234abcd, 0), '/').is_err());
}

#[test]
fn retained_tokens_do_not_keep_a_signed_out_interactive_session_alive() {
    let present = HashSet::from([0, 11]);
    assert!(interactive_logon_ended(10, 9, &present));
    assert!(interactive_logon_ended(2, 9, &present));
    assert!(!interactive_logon_ended(10, 11, &present));
    assert!(!interactive_logon_ended(5, 9, &present));
    assert!(!interactive_logon_ended(2, 0, &present));
}

#[test]
fn only_exact_encrypted_device_names_are_candidates() {
    assert_eq!(encrypted_slot(r"\Device\VeraCryptVolumeK"), Some(10));
    assert_eq!(encrypted_slot(r"\device\truecryptvolumez"), Some(25));
    for invalid in [
        r"\Device\HarddiskVolumeK",
        r"\Device\VeraCryptVolumeK\file",
        r"\Device\VeraCryptVolumeKK",
        r"\Device\VeraCryptVolume10",
        r"\Device\VeraCryptVolume",
        r"\??\K:",
    ] {
        assert_eq!(encrypted_slot(invalid), None);
    }
}

#[test]
fn cleanup_requires_repeated_ended_logon_and_missing_device_proof() {
    let deleted = Cell::new(false);
    let missing = Cell::new(0);
    let ended = Cell::new(0);
    let result = remove_if_still_stale(
        "target",
        || {
            ended.set(ended.get() + 1);
            Ok(true)
        },
        || {
            missing.set(missing.get() + 1);
            Ok(true)
        },
        || Ok("target".into()),
        || {
            deleted.set(true);
            Ok(())
        },
    );
    assert_eq!(result, Ok(true));
    assert_eq!(ended.get(), 2);
    assert_eq!(missing.get(), 2);
    assert!(deleted.get());
}

#[test]
fn live_unknown_or_racing_proofs_never_delete_a_link() {
    for failed_probe in 0..6 {
        let calls = Cell::new(0);
        let deleted = Cell::new(false);
        let result = remove_if_still_stale(
            "target",
            || {
                if failed_probe == 0 {
                    Ok(false)
                } else if failed_probe == 1 {
                    Err(())
                } else {
                    Ok(true)
                }
            },
            || {
                calls.set(calls.get() + 1);
                match failed_probe {
                    2 => Ok(false),
                    3 => Err(()),
                    4 => Ok(calls.get() == 1),
                    _ => Ok(true),
                }
            },
            || {
                Ok(if failed_probe == 5 {
                    "replacement"
                } else {
                    "target"
                }
                .into())
            },
            || {
                deleted.set(true);
                Ok(())
            },
        );
        assert_ne!(result, Ok(true));
        assert!(!deleted.get());
    }
}

#[test]
fn removal_failure_is_not_reported_as_success() {
    assert_eq!(
        remove_if_still_stale(
            "target",
            || Ok(true),
            || Ok(true),
            || Ok("target".into()),
            || Err(())
        ),
        Err(())
    );
}

#[test]
fn native_link_handle_pins_object_until_last_close() {
    use super::super::{Directory, ObjectAttributes};
    use windows_sys::Win32::{
        Foundation::{HANDLE, UNICODE_STRING},
        System::{
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            RemoteDesktop::ProcessIdToSessionId,
        },
    };
    type CreateLink = unsafe extern "system" fn(
        *mut HANDLE,
        u32,
        *mut ObjectAttributes,
        *mut UNICODE_STRING,
    ) -> i32;
    let mut session = 0;
    assert_ne!(
        unsafe { ProcessIdToSessionId(std::process::id(), &mut session) },
        0
    );
    let path = format!(
        r"\Sessions\{session}\BaseNamedObjects\WinCommander.LinkTest.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let mut name: Vec<u16> = path.encode_utf16().collect();
    let mut target: Vec<u16> = r"\Device\WinCommanderNonexistentTestTarget"
        .encode_utf16()
        .collect();
    let mut name_u = UNICODE_STRING {
        Length: (name.len() * 2) as u16,
        MaximumLength: (name.len() * 2) as u16,
        Buffer: name.as_mut_ptr(),
    };
    let mut target_u = UNICODE_STRING {
        Length: (target.len() * 2) as u16,
        MaximumLength: (target.len() * 2) as u16,
        Buffer: target.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: std::ptr::null_mut(),
        object_name: &mut name_u,
        attributes: 0x40,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut handle = std::ptr::null_mut();
    let dll: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
    let create: CreateLink = unsafe {
        std::mem::transmute(
            GetProcAddress(
                GetModuleHandleW(dll.as_ptr()),
                c"NtCreateSymbolicLinkObject".as_ptr().cast(),
            )
            .unwrap(),
        )
    };
    assert_eq!(
        unsafe { create(&mut handle, 0x0001_0001, &mut attributes, &mut target_u) },
        0
    );
    let creator = Directory(handle);
    let api = native::LinkApi::load().unwrap();
    let pinned = api.open(&path, true).unwrap().unwrap();
    drop(creator);
    assert_eq!(
        api.target(&pinned).unwrap(),
        r"\Device\WinCommanderNonexistentTestTarget"
    );
    api.remove(&pinned).unwrap();
    assert!(api.open(&path, false).unwrap().is_some());
    let mut replacement = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            create(
                &mut replacement,
                0x0001_0001,
                &mut attributes,
                &mut target_u,
            )
        } as u32,
        0xc0000035
    );
    drop(pinned);
    assert!(api.open(&path, false).unwrap().is_none());
}
