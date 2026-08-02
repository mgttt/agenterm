#[cfg(feature = "filesystem")]
use std::fs::OpenOptions;
use std::path::PathBuf;

use crate::filesystem::{FilesystemError, HostDirectories};

pub fn user_home_directory() -> Result<PathBuf, FilesystemError> {
    crate::filesystem::home_directory_from_env(std::env::var_os("USERPROFILE"))
}

#[cfg(feature = "filesystem")]
pub fn replace_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = std::fs::canonicalize(source)?;
    let destination = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| std::io::Error::other("destination parent required"))?,
    )?
    .join(
        destination
            .file_name()
            .ok_or_else(|| std::io::Error::other("destination name required"))?,
    );
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    const ATTEMPTS: usize = 32;
    for attempt in 0..ATTEMPTS {
        if unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0
        {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        let retryable = matches!(
            error.raw_os_error(),
            Some(code)
                if code == ERROR_ACCESS_DENIED as i32
                    || code == ERROR_SHARING_VIOLATION as i32
                    || code == ERROR_LOCK_VIOLATION as i32
        );
        if !retryable || attempt + 1 == ATTEMPTS {
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    unreachable!("bounded replacement loop always returns")
}

#[cfg(feature = "filesystem")]
pub fn sync_parent(_parent: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

pub fn host_directories() -> Result<HostDirectories, FilesystemError> {
    let config = std::env::var_os("APPDATA").map(PathBuf::from);
    let local_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    match (config, local_data) {
        (Some(config), Some(local_data)) => Ok(HostDirectories { config, local_data }),
        _ => Err(FilesystemError::Failed {
            code: "host_directory_unavailable",
            message: "APPDATA and LOCALAPPDATA must be available".to_owned(),
        }),
    }
}

pub fn executable_name(base: &str) -> String {
    format!("{base}.exe")
}

#[cfg(feature = "filesystem")]
pub fn protect_private_directory(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::{GENERIC_ALL, INVALID_HANDLE_VALUE, LocalFree},
        Security::{
            Authorization::{
                EXPLICIT_ACCESS_W, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW, SetSecurityInfo,
                TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
            SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        },
        Storage::FileSystem::{
            CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, READ_CONTROL,
            WRITE_DAC,
        },
    };

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            READ_CONTROL | WRITE_DAC,
            FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { std::fs::File::from_raw_handle(handle.cast()) };
    let facts = crate::filesystem_entry::opened_file_entry_facts(&file)?;
    if !facts.is_real_directory() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "private directory must be an existing real directory",
        ));
    }
    let identity = crate::user_identity::current_user_identity()?;
    let sid = identity.windows_sid().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows filesystem selected a non-SID user identity",
        )
    })?;
    let mut aligned_sid = vec![0_usize; sid.len().div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len());
    }
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: SET_ACCESS,
        grfInheritance: SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: aligned_sid.as_mut_ptr().cast(),
        },
    };
    let mut acl = null_mut();
    let result = unsafe { SetEntriesInAclW(1, &entry, null(), &mut acl) };
    if result != 0 {
        return Err(win32_error(result));
    }
    let result = unsafe {
        SetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null_mut(),
        )
    };
    unsafe { LocalFree(acl.cast()) };
    if result == 0 {
        Ok(())
    } else {
        Err(win32_error(result))
    }
}

#[cfg(feature = "filesystem")]
pub fn private_create_new_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    options
}

#[cfg(feature = "filesystem")]
fn win32_error(code: u32) -> std::io::Error {
    std::io::Error::from_raw_os_error(code as i32)
}

#[cfg(all(test, feature = "filesystem"))]
mod tests {
    use super::*;

    #[test]
    fn private_directory_acl_is_protected_and_current_user_only() {
        use std::os::windows::ffi::OsStrExt as _;
        use std::ptr::null_mut;
        use windows_sys::Win32::{
            Foundation::{GENERIC_ALL, LocalFree},
            Security::{
                Authorization::{
                    GRANT_ACCESS, GetExplicitEntriesFromAclW, GetNamedSecurityInfoW,
                    SE_FILE_OBJECT, SET_ACCESS, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN,
                    TRUSTEE_IS_USER,
                },
                DACL_SECURITY_INFORMATION, EqualSid, GetSecurityDescriptorControl,
                SE_DACL_PROTECTED, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            },
        };

        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-private-acl-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).expect("create ACL fixture");
        protect_private_directory(&directory).expect("protect ACL fixture");

        let wide = std::fs::canonicalize(&directory)
            .expect("canonical ACL fixture")
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        let result = unsafe {
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(result, 0, "read private ACL: {}", win32_error(result));
        let mut count = 0;
        let mut entries_ptr = null_mut();
        let result = unsafe { GetExplicitEntriesFromAclW(dacl, &mut count, &mut entries_ptr) };
        assert_eq!(result, 0, "expand private ACL: {}", win32_error(result));
        assert!(count > 0, "private ACL has no explicit ACE");
        let identity = crate::user_identity::current_user_identity().expect("current user SID");
        let sid = identity.windows_sid().expect("Windows SID identity");
        let mut aligned_sid = vec![0_usize; sid.len().div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len());
        }
        let entries = unsafe { std::slice::from_raw_parts(entries_ptr, count as usize) };
        let mut has_full_control = false;
        let mut inherited_scope = 0;
        for entry in entries {
            assert!(
                entry.grfAccessMode == SET_ACCESS || entry.grfAccessMode == GRANT_ACCESS,
                "private ACL contains a non-allow ACE mode {}",
                entry.grfAccessMode
            );
            assert_eq!(entry.Trustee.TrusteeForm, TRUSTEE_IS_SID);
            assert!(
                entry.Trustee.TrusteeType == TRUSTEE_IS_USER
                    || entry.Trustee.TrusteeType == TRUSTEE_IS_UNKNOWN,
                "private ACL contains unexpected trustee type {}",
                entry.Trustee.TrusteeType
            );
            assert_ne!(
                unsafe {
                    EqualSid(
                        entry.Trustee.ptstrName.cast(),
                        aligned_sid.as_mut_ptr().cast(),
                    )
                },
                0,
                "private ACL contains a trustee other than the current user"
            );
            has_full_control |= entry.grfAccessPermissions == GENERIC_ALL;
            inherited_scope |= entry.grfInheritance;
        }
        assert!(has_full_control, "private ACL does not grant generic all");
        assert_eq!(
            inherited_scope & SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            "private ACL does not cover child objects and directories"
        );
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) },
            0,
            "read security descriptor control"
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "DACL inheritance is not protected"
        );
        unsafe {
            LocalFree(entries_ptr.cast());
            LocalFree(descriptor);
        }
        std::fs::remove_dir_all(directory).expect("remove ACL fixture");
    }

    #[test]
    fn private_directory_acl_propagates_to_new_children() {
        use std::os::windows::ffi::OsStrExt as _;
        use std::ptr::null_mut;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION, AclSizeInformation,
                Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
                DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
                GetSecurityDescriptorDacl, INHERITED_ACE,
            },
            System::SystemServices::ACCESS_ALLOWED_ACE_TYPE,
        };

        let directory = std::env::temp_dir().join(format!(
            "agenterm-platform-private-acl-child-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir(&directory).expect("create ACL child fixture");
        protect_private_directory(&directory).expect("protect ACL child fixture");
        let child_directory = directory.join("child");
        std::fs::create_dir(&child_directory).expect("create protected child directory");
        let child_file = child_directory.join("state");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&child_file)
            .expect("create protected child file");

        let identity = crate::user_identity::current_user_identity().expect("current user SID");
        let sid = identity.windows_sid().expect("Windows SID identity");
        let mut aligned_sid = vec![0_usize; sid.len().div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            std::ptr::copy_nonoverlapping(sid.as_ptr(), aligned_sid.as_mut_ptr().cast(), sid.len());
        }

        for path in [&child_directory, &child_file] {
            let wide = std::fs::canonicalize(path)
                .expect("canonical protected child")
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let mut dacl = null_mut();
            let mut descriptor = null_mut();
            let result = unsafe {
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    &mut dacl,
                    null_mut(),
                    &mut descriptor,
                )
            };
            assert_eq!(result, 0, "read child ACL: {}", win32_error(result));
            let mut present = 0;
            let mut defaulted = 0;
            let mut child_dacl = null_mut();
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorDacl(
                        descriptor,
                        &mut present,
                        &mut child_dacl,
                        &mut defaulted,
                    )
                },
                0,
                "read child security descriptor DACL"
            );
            assert_ne!(present, 0, "protected child has no DACL");
            let mut size = ACL_SIZE_INFORMATION {
                AceCount: 0,
                AclBytesInUse: 0,
                AclBytesFree: 0,
            };
            assert_ne!(
                unsafe {
                    GetAclInformation(
                        child_dacl,
                        (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                        std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                        AclSizeInformation,
                    )
                },
                0,
                "read child ACL information"
            );
            assert!(size.AceCount > 0, "protected child ACL has no ACE");
            let mut inherited_user_ace = false;
            for index in 0..size.AceCount {
                let mut ace = null_mut();
                assert_ne!(unsafe { GetAce(child_dacl, index, &mut ace) }, 0);
                let header = unsafe { &*(ace.cast::<windows_sys::Win32::Security::ACE_HEADER>()) };
                if u32::from(header.AceType) == ACCESS_ALLOWED_ACE_TYPE
                    && header.AceFlags & INHERITED_ACE as u8 != 0
                {
                    let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                    let is_current_user = unsafe {
                        EqualSid(
                            (&allowed.SidStart as *const u32).cast_mut().cast(),
                            aligned_sid.as_mut_ptr().cast(),
                        ) != 0
                    };
                    if is_current_user {
                        // The parent test above verifies the source ACE's full
                        // control grant; this probe verifies that the same
                        // trustee ACE is materially inherited by each child.
                        inherited_user_ace = true;
                    }
                }
            }
            assert!(
                inherited_user_ace,
                "protected child has no inherited user ACE"
            );
            unsafe {
                LocalFree(descriptor);
            }
        }

        std::fs::remove_dir_all(directory).expect("remove ACL child fixture");
    }

    #[test]
    fn private_directory_acl_rejects_junction_without_touching_target() {
        use std::os::windows::ffi::OsStrExt as _;
        use std::ptr::null_mut;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, SE_DACL_PROTECTED,
            },
        };

        fn dacl_is_protected(path: &std::path::Path) -> bool {
            let wide = std::fs::canonicalize(path)
                .expect("canonical ACL target")
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let mut dacl = null_mut();
            let mut descriptor = null_mut();
            let result = unsafe {
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    &mut dacl,
                    null_mut(),
                    &mut descriptor,
                )
            };
            assert_eq!(result, 0, "read target ACL: {}", win32_error(result));
            let mut control = 0;
            let mut revision = 0;
            let ok =
                unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) };
            assert_ne!(ok, 0, "read target security descriptor control");
            unsafe {
                LocalFree(descriptor);
            }
            control & SE_DACL_PROTECTED != 0
        }

        let root = std::env::temp_dir().join(format!(
            "agenterm-platform-private-junction-{}",
            std::process::id()
        ));
        let target = root.join("target");
        let junction = root.join("junction");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&target).expect("create junction target");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run mklink junction fixture");
        assert!(status.success(), "mklink /J fixture failed: {status}");

        let target_protected_before = dacl_is_protected(&target);
        assert_eq!(
            protect_private_directory(&junction)
                .expect_err("private directory protection must reject a junction")
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            dacl_is_protected(&target),
            target_protected_before,
            "junction rejection changed the target ACL"
        );

        std::fs::remove_dir(&junction).expect("remove junction fixture");
        std::fs::remove_dir_all(root).expect("remove junction ACL fixture");
    }
}
