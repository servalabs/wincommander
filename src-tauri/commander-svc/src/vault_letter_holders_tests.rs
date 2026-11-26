// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

fn holder(kind: HolderKind, logon: Option<LogonId>, state: LogonState) -> LetterHolder {
    LetterHolder {
        logon,
        owner_sid: logon.map(|_| "S-1-5-21-1-2-3-1001".into()),
        session: logon.map(|_| 3),
        state,
        kind,
        device: None,
        device_present: true,
    }
}

#[test]
fn classifies_vault_ram_disk_windows_network_subst_and_unknown_targets() {
    for (target, kind) in [
        (r"\Device\VeraCryptVolumeT", HolderKind::VaultMount),
        (r"\Device\TrueCryptVolumeK", HolderKind::VaultMount),
        (r"\Device\ImDisk0", HolderKind::RamDisk),
        (r"\Device\ImDisk12", HolderKind::RamDisk),
        (r"\Device\HarddiskVolume5", HolderKind::WindowsVolume),
        (r"\Device\CdRom0", HolderKind::WindowsVolume),
        (
            r"\Device\LanmanRedirector\;Z:0000000000a1\server\share",
            HolderKind::NetworkDrive,
        ),
        (
            r"\Device\Mup\;T:0000000000a1\server\share",
            HolderKind::NetworkDrive,
        ),
        (r"\??\C:\Users\Lab\Folder", HolderKind::Subst),
        (r"\Device\ImDiskX", HolderKind::Other),
        (r"\Device\VeraCryptVolumeTT", HolderKind::Other),
        (r"\Device\Harddisk0\DR0", HolderKind::Other),
        (r"\GLOBAL??\Volume{0}", HolderKind::Other),
    ] {
        assert_eq!(holder_kind(target), kind, "{target}");
    }
}

#[test]
fn logon_state_separates_active_disconnected_signed_out_service_and_unknown() {
    let logon = (0x1234, 0);
    let sessions = HashMap::from([
        (
            3,
            SessionFacts {
                active: true,
                user_logons: vec![logon, (0x1235, 0)],
            },
        ),
        (
            4,
            SessionFacts {
                active: false,
                user_logons: vec![],
            },
        ),
        (
            6,
            SessionFacts {
                active: true,
                user_logons: vec![(0x9999, 0)],
            },
        ),
    ]);
    let state = |record| classify_state(logon, record, Some(&sessions));
    assert_eq!(state(Some((10, 3))), LogonState::Active);
    assert_eq!(state(Some((2, 4))), LogonState::Disconnected);
    assert_eq!(state(Some((10, 9))), LogonState::SignedOut, "session gone");
    assert_eq!(state(None), LogonState::SignedOut, "logon gone");
    assert_eq!(
        state(Some((10, 6))),
        LogonState::SignedOut,
        "reused RDP session id"
    );
    assert_eq!(
        state(Some((2, 6))),
        LogonState::Active,
        "RunAs logon in a live session"
    );
    assert_eq!(state(Some((5, 3))), LogonState::Service);
    assert_eq!(state(Some((10, 0))), LogonState::Service);
    assert_eq!(
        classify_state(logon, Some((10, 3)), None),
        LogonState::Unknown
    );
}

type RemovedLink = (char, Option<LogonId>);

const KING: &str = "S-1-5-21-1-2-3-1105";
const KING_LOGON: LogonId = (0x0ceed27f, 0);

/// The SERVAULT shape: King's disconnected logon holds T: -> VeraCryptVolumeT,
/// while the global namespace and many logons hold R: -> ImDisk0.
fn servault_links() -> Vec<(char, Option<LogonId>, Option<String>)> {
    let mut found = vec![(
        'T',
        Some(KING_LOGON),
        Some(r"\Device\VeraCryptVolumeT".to_owned()),
    )];
    found.push(('R', None, Some(r"\Device\ImDisk0".to_owned())));
    for logon in [
        KING_LOGON,
        (0x3e7, 0),
        (0x3e4, 0),
        (0x3e5, 0),
        (0x2a0001, 0),
    ] {
        found.push(('R', Some(logon), Some(r"\Device\ImDisk0".to_owned())));
    }
    found
}

fn servault_owner(logon: LogonId) -> OwnerFacts {
    if logon == KING_LOGON {
        (Some(KING.into()), Some(4), LogonState::Disconnected)
    } else {
        (Some("S-1-5-18".into()), Some(0), LogonState::Service)
    }
}

fn servault_release(devices: &[&str]) -> (HashMap<char, LetterRelease>, Vec<RemovedLink>) {
    let devices = devices
        .iter()
        .map(|name| name.to_string())
        .collect::<HashSet<_>>();
    let holders = build_holders(servault_links(), &devices, servault_owner);
    let mut removed = Vec::new();
    let releases = release_dead_links(holders, |letter, holder| {
        removed.push((letter, holder.logon));
        true
    });
    (releases, removed)
}

#[test]
fn disconnected_owners_link_to_a_missing_vault_device_is_stale_and_released() {
    let (releases, removed) = servault_release(&["imdisk0", "imdiskctl", "veracrypt"]);
    assert_eq!(removed, vec![('T', Some(KING_LOGON))]);
    let t = &releases[&'T'];
    assert_eq!(t.released(), 1);
    let (holder, released) = &t.checked[0];
    assert!(*released);
    assert_eq!(holder.kind, HolderKind::VaultMount);
    assert_eq!(holder.state, LogonState::Disconnected);
    assert_eq!(holder.owner_sid.as_deref(), Some(KING));
    assert_eq!(holder.device.as_deref(), Some("VeraCryptVolumeT"));
    assert!(!holder.device_present);
}

#[test]
fn the_same_link_is_kept_with_its_holder_while_the_vault_device_exists() {
    let (releases, removed) = servault_release(&["imdisk0", "veracryptvolumet"]);
    assert!(removed.iter().all(|(letter, _)| *letter != 'T'));
    let t = &releases[&'T'];
    assert_eq!(t.released(), 0);
    let holder = t.reported().unwrap();
    assert_eq!(holder.kind, HolderKind::VaultMount);
    assert!(holder.device_present);
    assert_eq!(holder.owner_sid.as_deref(), Some(KING));
    assert_eq!(holder.state, LogonState::Disconnected);
}

#[test]
fn every_ram_disk_link_to_a_live_imdisk0_is_kept_and_labelled_ram_disk() {
    let (releases, removed) = servault_release(&["imdisk0", "imdiskctl", "veracrypt"]);
    assert!(removed.iter().all(|(letter, _)| *letter != 'R'));
    let r = &releases[&'R'];
    assert_eq!(r.checked.len(), 6);
    assert!(r
        .checked
        .iter()
        .all(|(holder, released)| holder.kind == HolderKind::RamDisk
            && holder.device_present
            && !released));
    assert_eq!(r.reported().unwrap().state, LogonState::Machine);
}

#[test]
fn only_links_to_removed_vault_or_ram_disk_devices_are_released() {
    let found = vec![
        (
            'E',
            Some((1, 0)),
            Some(r"\Device\HarddiskVolume9".to_owned()),
        ),
        (
            'F',
            Some((1, 0)),
            Some(r"\Device\LanmanRedirector\;F:01\host\share".to_owned()),
        ),
        ('G', Some((1, 0)), Some(r"\??\C:\Missing\Folder".to_owned())),
        ('H', Some((1, 0)), None),
        ('I', None, Some(r"\Device\VeraCryptVolumeI".to_owned())),
        ('J', None, Some(r"\Device\ImDisk3".to_owned())),
    ];
    let holders = build_holders(found, &HashSet::new(), |_| {
        (Some(KING.into()), Some(9), LogonState::SignedOut)
    });
    let releases = release_dead_links(holders, |_, _| true);
    for (letter, expected) in [('E', 0), ('F', 0), ('G', 0), ('H', 0), ('I', 1), ('J', 1)] {
        assert_eq!(releases[&letter].released(), expected, "{letter}");
    }
    assert_eq!(releases[&'H'].checked[0].0.kind, HolderKind::Other);
}

#[test]
fn a_failed_removal_is_reported_as_kept() {
    let holders = build_holders(servault_links(), &HashSet::new(), servault_owner);
    let releases = release_dead_links(holders, |_, _| false);
    assert_eq!(releases[&'T'].released(), 0);
    assert!(releases[&'T'].reported().unwrap().is_dead_device());
}

#[test]
fn report_prefers_a_holder_that_still_blocks_the_letter() {
    let released = holder(HolderKind::VaultMount, Some((1, 0)), LogonState::SignedOut);
    let kept = holder(HolderKind::NetworkDrive, Some((2, 0)), LogonState::Active);
    let release = LetterRelease {
        checked: vec![(released.clone(), true), (kept.clone(), false)],
    };
    assert_eq!(release.released(), 1);
    assert_eq!(release.reported(), Some(&kept));
    let all = LetterRelease {
        checked: vec![(released.clone(), true)],
    };
    assert_eq!(all.reported(), Some(&released));
}

#[test]
fn diagnostics_name_the_link_kind_and_logon_state_without_identities() {
    let mut stale = holder(
        HolderKind::VaultMount,
        Some((0x2a, 0)),
        LogonState::SignedOut,
    );
    stale.device = Some("VeraCryptVolumeK".into());
    stale.device_present = false;
    let release = LetterRelease {
        checked: vec![(stale, true)],
    };
    let context = holder_diagnostics('T', &release, Some("S-1-5-21-1-2-3-1002"));
    let line = &context["T0"];
    for part in [
        "kind=vault_mount",
        "device=VeraCryptVolumeK",
        "present=false",
        "logon=00000000-0000002a",
        "state=signed_out",
        "session=3",
        "owner=other",
        "action=released",
    ] {
        assert!(line.contains(part), "{line} lacks {part}");
    }
    assert!(!line.contains("S-1-5"));
}

#[test]
fn dead_device_means_a_vault_or_ram_disk_link_without_its_device() {
    let mut candidate = holder(HolderKind::VaultMount, None, LogonState::Machine);
    assert!(!candidate.is_dead_device());
    candidate.device_present = false;
    assert!(candidate.is_dead_device());
    candidate.kind = HolderKind::WindowsVolume;
    assert!(!candidate.is_dead_device());
}

#[test]
#[ignore = "read-only Windows acceptance probe; run elevated as SYSTEM or administrator"]
fn live_read_only_letter_holder_inventory() {
    let letters = ('D'..='Z').collect::<HashSet<_>>();
    for (letter, holders) in letter_holders(&letters).expect("namespaces must be readable") {
        let release = LetterRelease {
            checked: holders.into_iter().map(|holder| (holder, false)).collect(),
        };
        eprintln!("{:?}", holder_diagnostics(letter, &release, None));
    }
}
