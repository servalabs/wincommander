// SPDX-License-Identifier: AGPL-3.0-or-later
//! Experimental current-user handle verifier for a pre-provisioned search
//! lease parent and lock. Provisioning creates only a missing lock and never
//! repairs or replaces an existing object's permissions.
//! Session binding, profile/ancestor authority, trusted generation selection,
//! rollback resistance and legacy-process exclusion are separate requirements.
use std::{fs::File, io, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, ERROR_NO_TOKEN},
    Security::{Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT},
        EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, GetTokenInformation,
        TokenUser, TOKEN_QUERY, TOKEN_USER, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION},
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
    Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT},
};
const DACL_PROTECTED:u16=0x1000;
fn denied()->io::Error { io::Error::new(io::ErrorKind::PermissionDenied,"search lease security unavailable") }
struct Descriptor(*mut core::ffi::c_void);
impl Drop for Descriptor { fn drop(&mut self) { unsafe { LocalFree(self.0); } } }
impl Descriptor {
    fn parse(value:&str)->io::Result<Self> {
        let text:Vec<u16>=value.encode_utf16().chain(Some(0)).collect();
        let mut result=ptr::null_mut();
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(),1,&mut result,ptr::null_mut()) }==0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(result))
    }
}
struct Token(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Token { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
fn effective_user_sid()->io::Result<String> {
    let mut raw=ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(),TOKEN_QUERY,1,&mut raw) }==0 {
        let error=io::Error::last_os_error();
        if error.raw_os_error()!=Some(ERROR_NO_TOKEN as i32) { return Err(error); }
        if unsafe { OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut raw) }==0 { return Err(io::Error::last_os_error()); }
    }
    let token=Token(raw);
    let mut needed=0;
    unsafe { GetTokenInformation(token.0,TokenUser,ptr::null_mut(),0,&mut needed); }
    if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed>65536 { return Err(denied()); }
    let mut data=vec![0usize;(needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token.0,TokenUser,data.as_mut_ptr().cast(),needed,&mut needed) }==0 { return Err(io::Error::last_os_error()); }
    let sid=unsafe { (*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let mut text=ptr::null_mut();
    if sid.is_null() || unsafe { ConvertSidToStringSidW(sid,&mut text) }==0 || text.is_null() { return Err(denied()); }
    let result=unsafe {
        let mut len=0; while *text.add(len)!=0 { len+=1; }
        let value=String::from_utf16(std::slice::from_raw_parts(text,len)).map_err(|_|denied());
        LocalFree(text.cast()); value?
    };
    if matches!(result.as_str(),"S-1-5-18"|"S-1-5-19"|"S-1-5-20") || result.starts_with("S-1-5-80-") { return Err(denied()); }
    Ok(result)
}
unsafe fn allowed_ace<'a>(raw:*const core::ffi::c_void)->Option<&'a ACCESS_ALLOWED_ACE> {
    let header=&*raw.cast::<ACE_HEADER>();
    let offset=std::mem::offset_of!(ACCESS_ALLOWED_ACE,SidStart);let size=header.AceSize as usize;
    if header.AceType!=0 || size<offset+8 { return None; }
    let sid=raw.cast::<u8>().add(offset);
    if *sid!=1 || *sid.add(1)>15 || offset+8+(*sid.add(1) as usize)*4>size { return None; }
    Some(&*raw.cast::<ACCESS_ALLOWED_ACE>())
}
unsafe fn acl_matches(actual:*mut ACL,expected:*mut ACL)->io::Result<bool> {
    if actual.is_null() || expected.is_null() || (*actual).AceCount!=(*expected).AceCount { return Ok(false); }
    let mut seen=vec![false;(*expected).AceCount as usize];
    for i in 0..(*actual).AceCount {
        let mut p=ptr::null_mut();if GetAce(actual,i as u32,&mut p)==0 || p.is_null() { return Err(denied()); }
        let Some(a)=allowed_ace(p) else { return Ok(false) };
        let mut matched=false;
        for j in 0..(*expected).AceCount {
            let mut q=ptr::null_mut();if GetAce(expected,j as u32,&mut q)==0 || q.is_null() { return Err(denied()); }
            let Some(e)=allowed_ace(q) else { return Ok(false) };
            if !seen[j as usize] && a.Header.AceFlags==e.Header.AceFlags && a.Mask==e.Mask
                && EqualSid(ptr::addr_of!(a.SidStart).cast_mut().cast(),ptr::addr_of!(e.SidStart).cast_mut().cast())!=0 {
                seen[j as usize]=true;matched=true;break;
            }
        }
        if !matched { return Ok(false); }
    }
    Ok(true)
}
/// Verify exact owner and protected DACL on an already opened parent or lock.
/// SYSTEM and Administrators are explicitly trusted administrative principals.
/// This does not establish that the handle belongs to the authorized profile.
pub fn verify_current_user_handle(file:&File)->io::Result<()> {
    let sid=effective_user_sid()?;
    let mut info=BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(),&mut info) }==0 { return Err(io::Error::last_os_error()); }
    let directory=info.dwFileAttributes & 0x10 != 0;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || (!directory && info.nNumberOfLinks!=1) { return Err(denied()); }
    let flags=if directory { "OICI" } else { "" };
    let expected=Descriptor::parse(&format!("O:{sid}D:P(A;{flags};FA;;;SY)(A;{flags};FA;;;BA)(A;{flags};FA;;;{sid})"))?;
    let mut owner=ptr::null_mut();let mut acl=ptr::null_mut();let mut descriptor=ptr::null_mut();
    let status=unsafe { GetSecurityInfo(file.as_raw_handle(),SE_FILE_OBJECT,OWNER_SECURITY_INFORMATION|DACL_SECURITY_INFORMATION,
        &mut owner,ptr::null_mut(),&mut acl,ptr::null_mut(),&mut descriptor) };
    if status!=0 { return Err(io::Error::from_raw_os_error(status as i32)); }
    if descriptor.is_null() { return Err(denied()); }
    let actual=Descriptor(descriptor);
    let mut control=0;let mut revision=0;let mut expected_owner=ptr::null_mut();let mut owner_defaulted=0;
    let mut expected_acl=ptr::null_mut();let mut present=0;let mut defaulted=0;
    unsafe {
        if GetSecurityDescriptorControl(actual.0,&mut control,&mut revision)==0
            || GetSecurityDescriptorOwner(expected.0,&mut expected_owner,&mut owner_defaulted)==0
            || GetSecurityDescriptorDacl(expected.0,&mut present,&mut expected_acl,&mut defaulted)==0 { return Err(denied()); }
        if control&DACL_PROTECTED==0 || owner.is_null() || expected_owner.is_null() || present==0
            || EqualSid(owner,expected_owner)==0 || !acl_matches(acl,expected_acl)? { return Err(denied()); }
    }
    Ok(())
}

/// Explicit experimental provisioning under an already-authorized profile.
/// Profile ancestry and active same-user interactive session must be proven by
/// the caller. Existing locks are verified in place, never changed or removed.
pub fn provision_generation_lock(parent:&std::path::Path,generation:[u8;16])->io::Result<()> {
    use std::os::windows::{ffi::OsStrExt,io::FromRawHandle};
    use windows_sys::Win32::{Foundation::{ERROR_ALREADY_EXISTS,ERROR_FILE_EXISTS,INVALID_HANDLE_VALUE},
        Security::SECURITY_ATTRIBUTES,Storage::FileSystem::{CreateFileW,CREATE_NEW,OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,FILE_FLAG_OPEN_REPARSE_POINT,FILE_SHARE_READ,FILE_SHARE_WRITE}};
    if generation==[0;16] { return Err(denied()); }
    let pins=crate::read_only::CheckedDirectory::pin_root_path(parent)?;
    verify_current_user_handle(pins.last().ok_or_else(denied)?)?;
    let sid=effective_user_sid()?;
    let descriptor=Descriptor::parse(&format!("O:{sid}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})"))?;
    let mut attributes=SECURITY_ATTRIBUTES { nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor:descriptor.0,bInheritHandle:0 };
    let path:Vec<u16>=parent.join(format!(".ordinary-search-generation-{}.lock",hex::encode(generation)))
        .as_os_str().encode_wide().chain(Some(0)).collect();
    let access=0x8000_0000|0x4000_0000;
    let share=FILE_SHARE_READ|FILE_SHARE_WRITE;
    let flags=FILE_ATTRIBUTE_NORMAL|FILE_FLAG_OPEN_REPARSE_POINT;
    let mut handle=unsafe { CreateFileW(path.as_ptr(),access,share,&mut attributes,CREATE_NEW,flags,ptr::null_mut()) };
    if handle==INVALID_HANDLE_VALUE {
        let error=io::Error::last_os_error();
        if !matches!(error.raw_os_error(),Some(v) if v==ERROR_ALREADY_EXISTS as i32 || v==ERROR_FILE_EXISTS as i32) { return Err(error); }
        handle=unsafe { CreateFileW(path.as_ptr(),access,share,ptr::null(),OPEN_EXISTING,flags,ptr::null_mut()) };
        if handle==INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
    }
    // SAFETY: CreateFileW returned one uniquely owned open handle.
    let file=unsafe { File::from_raw_handle(handle) };
    verify_current_user_handle(&file)?;
    verify_current_user_handle(pins.last().ok_or_else(denied)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn acl(descriptor:&Descriptor)->*mut ACL {
        let mut result=ptr::null_mut();let mut present=0;let mut defaulted=0;
        assert_ne!(GetSecurityDescriptorDacl(descriptor.0,&mut present,&mut result,&mut defaulted),0);
        assert_ne!(present,0);result
    }
    #[test]
    fn exact_acl_rejects_extra_and_overprivileged_principals() {
        let expected=Descriptor::parse("O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)").unwrap();
        let extra=Descriptor::parse("O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)(A;;FR;;;AU)").unwrap();
        let elevated=Descriptor::parse("O:BAG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;BU)").unwrap();
        unsafe { assert!(acl_matches(acl(&expected),acl(&expected)).unwrap());
            assert!(!acl_matches(acl(&extra),acl(&expected)).unwrap());
            assert!(!acl_matches(acl(&elevated),acl(&expected)).unwrap()); }
    }
    #[test]
    fn malformed_ace_never_reaches_sid_comparison() {
        let mut storage=[0u32;8];let header=storage.as_mut_ptr().cast::<ACE_HEADER>();
        unsafe { (*header).AceType=0;(*header).AceSize=8;assert!(allowed_ace(header.cast()).is_none());
            (*header).AceSize=16;let sid=header.cast::<u8>().add(std::mem::offset_of!(ACCESS_ALLOWED_ACE,SidStart));
            *sid=1;*sid.add(1)=2;assert!(allowed_ace(header.cast()).is_none()); }
    }
    #[test]
    fn opened_file_requires_exact_protected_current_user_acl() {
        use std::os::windows::{ffi::OsStrExt,io::FromRawHandle};
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE,Security::SECURITY_ATTRIBUTES,
            Storage::FileSystem::{CreateFileW,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,FILE_FLAG_OPEN_REPARSE_POINT,FILE_SHARE_READ,FILE_SHARE_WRITE}};
        let sid=effective_user_sid().unwrap();
        let dir=tempfile::tempdir().unwrap();
        for (name,sddl,expected) in [
            ("permitted",format!("O:{sid}D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})"),true),
            ("foreign","D:P(A;;FA;;;WD)".to_owned(),false),
        ] {
            let descriptor=Descriptor::parse(&sddl).unwrap();
            let mut attributes=SECURITY_ATTRIBUTES { nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor:descriptor.0,bInheritHandle:0 };
            let path:Vec<u16>=dir.path().join(name).as_os_str().encode_wide().chain(Some(0)).collect();
            let handle=unsafe { CreateFileW(path.as_ptr(),0x8000_0000|0x4000_0000,FILE_SHARE_READ|FILE_SHARE_WRITE,
                &mut attributes,CREATE_NEW,FILE_ATTRIBUTE_NORMAL|FILE_FLAG_OPEN_REPARSE_POINT,ptr::null_mut()) };
            assert_ne!(handle,INVALID_HANDLE_VALUE);
            let file=unsafe { File::from_raw_handle(handle) };
            assert_eq!(verify_current_user_handle(&file).is_ok(),expected);
        }
    }
    #[test]
    fn provisioning_is_idempotent_and_preserves_foreign_lock() {
        use std::os::windows::{ffi::OsStrExt,io::FromRawHandle};
        use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE,Security::SECURITY_ATTRIBUTES,
            Storage::FileSystem::{CreateDirectoryW,CreateFileW,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,FILE_FLAG_OPEN_REPARSE_POINT,FILE_SHARE_READ,FILE_SHARE_WRITE}};
        let sid=effective_user_sid().unwrap();let temp=tempfile::tempdir().unwrap();let parent=temp.path().join("protected");
        let descriptor=Descriptor::parse(&format!("O:{sid}D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{sid})")).unwrap();
        let mut attributes=SECURITY_ATTRIBUTES { nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,lpSecurityDescriptor:descriptor.0,bInheritHandle:0 };
        let parent_path:Vec<u16>=parent.as_os_str().encode_wide().chain(Some(0)).collect();
        assert_ne!(unsafe { CreateDirectoryW(parent_path.as_ptr(),&mut attributes) },0);
        provision_generation_lock(&parent,[1;16]).unwrap();provision_generation_lock(&parent,[1;16]).unwrap();
        let lease=crate::generation_lease::GenerationLease::shared(&parent,[1;16],verify_current_user_handle).unwrap();drop(lease);
        let foreign=parent.join(format!(".ordinary-search-generation-{}.lock",hex::encode([2;16])));
        let other=Descriptor::parse("D:P(A;;FA;;;WD)").unwrap();attributes.lpSecurityDescriptor=other.0;
        let path:Vec<u16>=foreign.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle=unsafe { CreateFileW(path.as_ptr(),0x8000_0000|0x4000_0000,FILE_SHARE_READ|FILE_SHARE_WRITE,
            &mut attributes,CREATE_NEW,FILE_ATTRIBUTE_NORMAL|FILE_FLAG_OPEN_REPARSE_POINT,ptr::null_mut()) };
        assert_ne!(handle,INVALID_HANDLE_VALUE);
        let mut file=unsafe { File::from_raw_handle(handle) };
        use std::io::Write;file.write_all(b"preserve").unwrap();drop(file);
        assert!(provision_generation_lock(&parent,[2;16]).is_err());
        assert_eq!(std::fs::read(foreign).unwrap(),b"preserve");
    }
}
