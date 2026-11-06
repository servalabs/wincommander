// SPDX-License-Identifier: AGPL-3.0-or-later
//! UNSAFE EXPERIMENT; DO NOT WIRE INTO PRODUCT. A restricted-token Windows
//! fixture installed a junction using FILE_WRITE_ATTRIBUTES (0x100) while a
//! no-write/no-delete-share directory handle was held. Pins do not establish
//! namespace confinement; handle-relative operations or a stronger boundary
//! are required. No cleanup or cutover is implemented.
use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SE_FILE_OBJECT,
        },
        EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetSecurityDescriptorOwner, GetTokenInformation, TokenUser, ACCESS_ALLOWED_ACE, ACE_HEADER,
        ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
        TOKEN_QUERY, TOKEN_USER,
    },
    Storage::FileSystem::{
        CreateDirectoryW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
const DACL_PROTECTED: u16 = 0x1000;
struct Descriptor(*mut core::ffi::c_void);
impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut value = std::ptr::null_mut();
        // SAFETY: text is NUL terminated and Windows initializes value on success.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                1,
                &mut value,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(value))
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
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

// Validate the SID fits inside the ACE before EqualSid may inspect it.
unsafe fn checked_allowed_ace<'a>(ptr: *const core::ffi::c_void) -> Option<&'a ACCESS_ALLOWED_ACE> {
    let header = &*ptr.cast::<ACE_HEADER>();
    let offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
    let size = header.AceSize as usize;
    if header.AceType != 0 || size < offset + 8 {
        return None;
    }
    let sid = ptr.cast::<u8>().add(offset);
    let revision = *sid;
    let subauthorities = *sid.add(1) as usize;
    if revision != 1 || subauthorities > 15 || offset + 8 + subauthorities * 4 > size {
        return None;
    }
    Some(&*ptr.cast::<ACCESS_ALLOWED_ACE>())
}

// Compare the actual ACEs with the descriptor built from SDDL, regardless of
// ordering. Reject unknown ACE forms, inherited/extra permissions and owners.
unsafe fn acl_matches(actual: *mut ACL, expected: *mut ACL) -> io::Result<bool> {
    if actual.is_null() || expected.is_null() || (*actual).AceCount != (*expected).AceCount {
        return Ok(false);
    }
    let mut seen = vec![false; (*expected).AceCount as usize];
    for i in 0..(*actual).AceCount {
        let mut p = std::ptr::null_mut();
        if GetAce(actual, i as u32, &mut p) == 0 || p.is_null() {
            return Err(io::Error::last_os_error());
        }
        let Some(a) = checked_allowed_ace(p) else {
            return Ok(false);
        };
        let mut matched = false;
        for j in 0..(*expected).AceCount {
            let mut q = std::ptr::null_mut();
            if GetAce(expected, j as u32, &mut q) == 0 || q.is_null() {
                return Err(io::Error::last_os_error());
            }
            let Some(e) = checked_allowed_ace(q) else {
                return Ok(false);
            };
            if !seen[j as usize]
                && a.Header.AceFlags == e.Header.AceFlags
                && a.Mask == e.Mask
                && EqualSid(
                    std::ptr::addr_of!(a.SidStart).cast_mut().cast(),
                    std::ptr::addr_of!(e.SidStart).cast_mut().cast(),
                ) != 0
            {
                seen[j as usize] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

fn verify(file: &File, expected: &Descriptor, expect_directory: bool) -> io::Result<()> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: all handles and output structures remain live through calls.
    unsafe {
        if GetFileInformationByHandle(file.as_raw_handle(), &mut info) == 0 {
            return Err(io::Error::last_os_error());
        }
        let is_directory = info.dwFileAttributes & 0x10 != 0;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || is_directory != expect_directory
            || (!expect_directory && info.nNumberOfLinks != 1)
        {
            return Err(io::Error::other("invalid staging security object"));
        }
        let mut owner = std::ptr::null_mut();
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut raw_descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut raw_descriptor,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let actual = Descriptor(raw_descriptor);
        let mut control = 0u16;
        let mut revision = 0u32;
        if GetSecurityDescriptorControl(actual.0, &mut control, &mut revision) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut expected_owner = std::ptr::null_mut();
        let mut owner_defaulted = 0;
        let mut expected_acl = std::ptr::null_mut();
        let mut present = 0;
        let mut defaulted = 0;
        if GetSecurityDescriptorOwner(expected.0, &mut expected_owner, &mut owner_defaulted) == 0
            || GetSecurityDescriptorDacl(
                expected.0,
                &mut present,
                &mut expected_acl,
                &mut defaulted,
            ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        if control & DACL_PROTECTED == 0
            || owner.is_null()
            || expected_owner.is_null()
            || present == 0
            || EqualSid(owner, expected_owner) == 0
            || !acl_matches(acl, expected_acl)?
        {
            return Err(io::Error::other(
                "staging directory permissions are not authorized",
            ));
        }
    }
    Ok(())
}

pub(super) fn current_user_sid() -> io::Result<String> {
    let mut token = std::ptr::null_mut();
    // SAFETY: output token is initialized by Windows on success.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct Token(windows_sys::Win32::Foundation::HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let token = Token(token);
    let mut needed = 0u32;
    unsafe {
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 65_536 {
        return Err(io::Error::other("cannot determine current Windows user"));
    }
    let mut storage = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let sid = unsafe { (*(storage.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let mut text = std::ptr::null_mut();
    if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 || text.is_null() {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0usize;
    // SAFETY: Windows returns an allocated, NUL-terminated SID string.
    let result = unsafe {
        while *text.add(len) != 0 {
            len += 1;
        }
        let result = String::from_utf16(std::slice::from_raw_parts(text, len))
            .map_err(|_| io::Error::other("invalid current Windows user"));
        LocalFree(text.cast());
        result
    };
    result
}
fn current_user_descriptor(directory: bool) -> io::Result<Descriptor> {
    let sid = current_user_sid()?;
    let flags = if directory { "OICI" } else { "" };
    Descriptor::from_sddl(&format!(
        "O:{sid}D:P(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)(A;{flags};FA;;;{sid})"
    ))
}

/// The stage is private by construction and is kept pinned with its parents.
/// Callers must separately verify that `parent` is the authenticated user's
/// intended profile root. Do not copy user data before that verification.
pub(crate) struct PrivateStage {
    pub(crate) path: PathBuf,
    _pins: Vec<File>,
    _stage: File,
}

/// Resolve the user's LocalAppData from the Windows known-folder API, not an
/// environment variable. Creation still pins and rejects reparse ancestors.
pub(crate) fn create_in_current_user_profile() -> io::Result<PrivateStage> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
    };
    let mut raw = std::ptr::null_mut();
    let status =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, std::ptr::null_mut(), &mut raw) };
    if status < 0 || raw.is_null() {
        if !raw.is_null() {
            unsafe { CoTaskMemFree(raw.cast()) };
        }
        return Err(io::Error::other("current-user staging folder unavailable"));
    }
    let mut length = 0;
    let path = unsafe {
        while *raw.add(length) != 0 {
            length += 1;
        }
        let path = std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, length));
        CoTaskMemFree(raw.cast());
        PathBuf::from(path)
    };
    create_in_authorized_parent(&path)
}

pub(crate) fn create_in_authorized_parent(parent: &Path) -> io::Result<PrivateStage> {
    if !parent.is_absolute() {
        return Err(io::Error::other("staging parent must be absolute"));
    }
    let mut ancestors: Vec<_> = parent.ancestors().collect();
    ancestors.reverse();
    let mut pins = Vec::new();
    for ancestor in ancestors {
        let share = if ancestor.parent().is_none() {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        } else {
            FILE_SHARE_READ
        };
        let file = OpenOptions::new()
            .read(true)
            .share_mode(share)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(ancestor)?;
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "staging ancestor is not a normal directory",
            ));
        }
        pins.push(file);
    }
    let name = format!(".wincommander-private-paste-{}", uuid::Uuid::new_v4());
    let path = parent.join(name);
    let descriptor = current_user_descriptor(true)?;
    let attributes = descriptor.attributes();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let stage = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)?;
    verify(&stage, &descriptor, true)?;
    Ok(PrivateStage {
        path,
        _pins: pins,
        _stage: stage,
    })
}
