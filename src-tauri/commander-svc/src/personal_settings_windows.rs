// SPDX-License-Identifier: AGPL-3.0-or-later
use super::{StoreResult, UNAVAILABLE};
use std::fs::File;
use std::io;
use std::os::windows::{
    ffi::OsStrExt,
    io::{AsRawHandle, FromRawHandle},
};
use std::path::{Path, PathBuf};
use windows_sys::Win32::{
    Foundation::{LocalFree, ERROR_ALREADY_EXISTS, INVALID_HANDLE_VALUE},
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT,
        },
        CreateWellKnownSid,
        Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE,
            CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
        EqualSid, GetAce, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACL,
        DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, GetFileInformationByHandle, MoveFileExW,
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_READ, FILE_SHARE_WRITE, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        OPEN_EXISTING,
    },
    System::Com::CoTaskMemFree,
    UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
};

const READ_CONTROL: u32 = 0x0002_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const SECURE_SDDL: &str = "O:BAG:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

struct Descriptor(*mut core::ffi::c_void);
impl Descriptor {
    fn new() -> io::Result<Self> {
        let sddl: Vec<u16> = SECURE_SDDL.encode_utf16().chain(Some(0)).collect();
        let mut ptr = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut ptr,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(ptr))
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

pub(super) fn store_root() -> StoreResult<PathBuf> {
    let mut ptr = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, std::ptr::null_mut(), &mut ptr) } < 0
    {
        return Err(UNAVAILABLE);
    }
    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let path = String::from_utf16(std::slice::from_raw_parts(ptr, len));
        CoTaskMemFree(ptr.cast());
        Ok(PathBuf::from(path.map_err(|_| UNAVAILABLE)?).join("WinCommanderPersonalSettings"))
    }
}

pub(super) fn secure_directory(root: &Path) -> StoreResult<Vec<File>> {
    let mut guards = Vec::new();
    // Keeping each ancestor open without DELETE sharing prevents path substitution.
    let ancestors: Vec<_> = root.ancestors().skip(1).collect();
    for ancestor in ancestors.into_iter().rev() {
        guards.push(open_directory(ancestor).map_err(|_| UNAVAILABLE)?);
    }
    let descriptor = Descriptor::new().map_err(|_| UNAVAILABLE)?;
    let attributes = descriptor.attributes();
    if unsafe { CreateDirectoryW(wide(root).as_ptr(), &attributes) } == 0
        && io::Error::last_os_error().raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32)
    {
        return Err(UNAVAILABLE);
    }
    let directory = open_directory(root).map_err(|_| UNAVAILABLE)?;
    verify_private(&directory).map_err(|_| UNAVAILABLE)?;
    guards.push(directory);
    Ok(guards)
}

fn open_directory(path: &Path) -> io::Result<File> {
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    verify_kind(&file, true)?;
    Ok(file)
}

fn verify_kind(file: &File, directory: bool) -> io::Result<()> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    Ok(())
}

fn verify_private(file: &File) -> io::Result<()> {
    unsafe {
        let mut owner = std::ptr::null_mut();
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let _guard = Descriptor(descriptor);
        let mut system = [0u32; 17];
        let mut admins = [0u32; 17];
        let mut size = 68;
        if CreateWellKnownSid(
            WinLocalSystemSid,
            std::ptr::null_mut(),
            system.as_mut_ptr().cast(),
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        size = 68;
        if CreateWellKnownSid(
            WinBuiltinAdministratorsSid,
            std::ptr::null_mut(),
            admins.as_mut_ptr().cast(),
            &mut size,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let trusted = |sid| {
            EqualSid(sid, system.as_ptr().cast_mut().cast()) != 0
                || EqualSid(sid, admins.as_ptr().cast_mut().cast()) != 0
        };
        if owner.is_null() || !trusted(owner) || acl.is_null() || (*acl).AceCount == 0 {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        for index in 0..(*acl).AceCount {
            let mut ace = std::ptr::null_mut();
            if GetAce(acl, index as u32, &mut ace) == 0 {
                return Err(io::Error::last_os_error());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            if ace.Header.AceType != 0
                || !trusted(std::ptr::addr_of!(ace.SidStart).cast_mut().cast())
            {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
        }
    }
    Ok(())
}

pub(super) fn open_record(path: &Path, create: bool) -> io::Result<File> {
    let descriptor = Descriptor::new()?;
    let attributes = descriptor.attributes();
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | READ_CONTROL | if create { GENERIC_WRITE } else { 0 },
            0,
            &attributes,
            if create { CREATE_NEW } else { OPEN_EXISTING },
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    verify_kind(&file, false)?;
    verify_private(&file)?;
    Ok(file)
}

pub(super) fn replace(source: &Path, destination: &Path) -> StoreResult<()> {
    if unsafe {
        MoveFileExW(
            wide(source).as_ptr(),
            wide(destination).as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(UNAVAILABLE);
    }
    Ok(())
}

pub(super) fn dpapi(input: &[u8], sid: &str, protect: bool) -> StoreResult<Vec<u8>> {
    let entropy = format!("WinCommander.PersonalSettings.v1:{sid}");
    let source = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr().cast_mut(),
    };
    let entropy = CRYPT_INTEGER_BLOB {
        cbData: entropy.len() as u32,
        pbData: entropy.as_ptr().cast_mut(),
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    unsafe {
        let ok = if protect {
            CryptProtectData(
                &source,
                std::ptr::null(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &source,
                std::ptr::null_mut(),
                &entropy,
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(UNAVAILABLE);
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        if !protect {
            zeroize::Zeroize::zeroize(std::slice::from_raw_parts_mut(
                output.pbData,
                output.cbData as usize,
            ));
        }
        LocalFree(output.pbData.cast());
        Ok(result)
    }
}
