use std::{os::windows::ffi::OsStrExt as _, path::Path, ptr::null_mut};

use windows_sys::Win32::{
    Foundation::{GENERIC_EXECUTE, GENERIC_READ, GetLastError, LocalFree},
    Security::{
        ACL,
        Authorization::{
            EXPLICIT_ACCESS_W, GetNamedSecurityInfoW, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW,
            SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
        },
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, GetLengthSid, IsValidSid, NO_INHERITANCE,
        PSECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        WinBuiltinAnyPackageSid,
    },
};

use crate::{
    directory_access::{
        DirectoryAccessError, DirectoryAccessErrorKind, DirectoryAccessReport, DirectoryPrincipal,
        DirectoryTreeAccess, WellKnownDirectoryPrincipal,
    },
    filesystem_entry,
};

const OPERATION: &str = "grant-directory-tree-access";
const FILE_GENERIC_READ: u32 = 0x0012_0089;
const FILE_GENERIC_WRITE: u32 = 0x0012_0116;
const FILE_GENERIC_EXECUTE: u32 = 0x0012_00A0;
const DELETE: u32 = 0x0001_0000;
const FILE_DELETE_CHILD: u32 = 0x0000_0040;
const ACCESS_MODIFY_CONTENTS: u32 =
    FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE | FILE_DELETE_CHILD;

pub(crate) fn grant_directory_tree_access(
    root: &Path,
    principal: DirectoryPrincipal<'_>,
    access: DirectoryTreeAccess,
) -> Result<DirectoryAccessReport, DirectoryAccessError> {
    let sid = principal_sid(root, principal)?;
    let metadata = metadata(root)?;
    if filesystem_entry::metadata_is_link_like(&metadata) {
        return Err(error(
            DirectoryAccessErrorKind::LinkLikeRoot,
            root,
            None,
            "root is link-like",
        ));
    }
    if !metadata.is_dir() {
        return Err(error(
            DirectoryAccessErrorKind::NotDirectory,
            root,
            None,
            "root is not a directory",
        ));
    }
    let mut report = DirectoryAccessReport::default();
    grant_tree(root, &sid, access_mask(access), &mut report)?;
    Ok(report)
}

fn grant_tree(
    path: &Path,
    sid: &[usize],
    access: u32,
    report: &mut DirectoryAccessReport,
) -> Result<(), DirectoryAccessError> {
    let metadata = metadata(path)?;
    if filesystem_entry::metadata_is_link_like(&metadata) {
        report.link_like_entries_skipped += 1;
        return Ok(());
    }
    let is_dir = metadata.is_dir();
    grant_entry(path, is_dir, sid, access)?;
    report.entries_updated += 1;
    if is_dir {
        let entries =
            std::fs::read_dir(path).map_err(|source| io_error("read-directory", path, source))?;
        for entry in entries {
            let entry = entry.map_err(|source| io_error("read-directory-entry", path, source))?;
            grant_tree(&entry.path(), sid, access, report)?;
        }
    }
    Ok(())
}

fn principal_sid(
    root: &Path,
    principal: DirectoryPrincipal<'_>,
) -> Result<Vec<usize>, DirectoryAccessError> {
    match principal {
        DirectoryPrincipal::WindowsSid(sid) => aligned_sid(root, sid),
        DirectoryPrincipal::WellKnown(WellKnownDirectoryPrincipal::AllApplicationPackages) => {
            let capacity = SECURITY_MAX_SID_SIZE as usize;
            let mut storage = vec![0usize; words_for_bytes(capacity)];
            let mut len = capacity as u32;
            if unsafe {
                CreateWellKnownSid(
                    WinBuiltinAnyPackageSid,
                    null_mut(),
                    storage.as_mut_ptr().cast(),
                    &raw mut len,
                )
            } == 0
            {
                return Err(error(
                    DirectoryAccessErrorKind::NativeFailure,
                    root,
                    Some(unsafe { GetLastError() }),
                    "CreateWellKnownSid failed",
                ));
            }
            storage.truncate(words_for_bytes(len as usize));
            Ok(storage)
        }
    }
}

fn aligned_sid(root: &Path, sid: &[u8]) -> Result<Vec<usize>, DirectoryAccessError> {
    if sid.is_empty() {
        return Err(error(
            DirectoryAccessErrorKind::InvalidInput,
            root,
            None,
            "SID is empty",
        ));
    }
    let mut storage = vec![0usize; words_for_bytes(sid.len())];
    unsafe { std::ptr::copy_nonoverlapping(sid.as_ptr(), storage.as_mut_ptr().cast(), sid.len()) };
    let raw = storage.as_ptr().cast_mut().cast();
    if unsafe { IsValidSid(raw) } == 0 || unsafe { GetLengthSid(raw) } as usize != sid.len() {
        return Err(error(
            DirectoryAccessErrorKind::InvalidInput,
            root,
            None,
            "SID bytes are invalid",
        ));
    }
    Ok(storage)
}

fn grant_entry(
    path: &Path,
    is_dir: bool,
    sid: &[usize],
    access: u32,
) -> Result<(), DirectoryAccessError> {
    let wide = wide_path(path)?;
    let mut old_dacl: *mut ACL = null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let code = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &raw mut old_dacl,
            null_mut(),
            &raw mut descriptor,
        )
    };
    if code != 0 {
        return Err(error(
            DirectoryAccessErrorKind::NativeFailure,
            path,
            Some(code),
            "GetNamedSecurityInfoW failed",
        ));
    }
    let _descriptor = LocalGuard(descriptor.cast());
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: access,
        grfAccessMode: SET_ACCESS,
        grfInheritance: if is_dir {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        },
        Trustee: TRUSTEE_W {
            pMultipleTrustee: null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_UNKNOWN,
            ptstrName: sid.as_ptr().cast_mut().cast(),
        },
    };
    let mut new_dacl: *mut ACL = null_mut();
    let code = unsafe { SetEntriesInAclW(1, &entry, old_dacl, &raw mut new_dacl) };
    if code != 0 {
        return Err(error(
            DirectoryAccessErrorKind::NativeFailure,
            path,
            Some(code),
            "SetEntriesInAclW failed",
        ));
    }
    let _new_dacl = LocalGuard(new_dacl.cast());
    let code = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_dacl,
            null_mut(),
        )
    };
    if code != 0 {
        return Err(error(
            DirectoryAccessErrorKind::NativeFailure,
            path,
            Some(code),
            "SetNamedSecurityInfoW failed",
        ));
    }
    Ok(())
}

const fn access_mask(access: DirectoryTreeAccess) -> u32 {
    match access {
        DirectoryTreeAccess::ReadExecute => GENERIC_READ | GENERIC_EXECUTE,
        DirectoryTreeAccess::ModifyContents => ACCESS_MODIFY_CONTENTS,
    }
}

fn metadata(path: &Path) -> Result<std::fs::Metadata, DirectoryAccessError> {
    std::fs::symlink_metadata(path).map_err(|source| io_error("read-metadata", path, source))
}

fn wide_path(path: &Path) -> Result<Vec<u16>, DirectoryAccessError> {
    if path.as_os_str().encode_wide().any(|unit| unit == 0) {
        return Err(error(
            DirectoryAccessErrorKind::InvalidInput,
            path,
            None,
            "path contains NUL",
        ));
    }
    Ok(path.as_os_str().encode_wide().chain(Some(0)).collect())
}

fn io_error(operation: &'static str, path: &Path, source: std::io::Error) -> DirectoryAccessError {
    DirectoryAccessError::new(
        DirectoryAccessErrorKind::Io,
        operation,
        path,
        source.raw_os_error().map(|code| code as u32),
        source.to_string(),
    )
}

fn error(
    kind: DirectoryAccessErrorKind,
    path: &Path,
    native_code: Option<u32>,
    detail: &'static str,
) -> DirectoryAccessError {
    DirectoryAccessError::new(kind, OPERATION, path, native_code, detail)
}

const fn words_for_bytes(bytes: usize) -> usize {
    bytes.div_ceil(std::mem::size_of::<usize>())
}

struct LocalGuard(*mut std::ffi::c_void);

impl Drop for LocalGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modify_contents_cannot_change_owner_or_acl() {
        const WRITE_DAC: u32 = 0x0004_0000;
        const WRITE_OWNER: u32 = 0x0008_0000;
        assert_ne!(ACCESS_MODIFY_CONTENTS & FILE_GENERIC_WRITE, 0);
        assert_ne!(ACCESS_MODIFY_CONTENTS & DELETE, 0);
        assert_eq!(ACCESS_MODIFY_CONTENTS & WRITE_DAC, 0);
        assert_eq!(ACCESS_MODIFY_CONTENTS & WRITE_OWNER, 0);
    }

    #[test]
    fn grants_tree_and_skips_junction() {
        let tag = std::process::id();
        let root = std::env::temp_dir().join(format!("agenterm-directory-access-{tag}"));
        let outside = std::env::temp_dir().join(format!("agenterm-directory-access-outside-{tag}"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(root.join("nested")).expect("create root fixture");
        std::fs::write(root.join("nested/file"), b"inside").expect("write root fixture");
        std::fs::create_dir(&outside).expect("create outside fixture");
        let junction = root.join("outside");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("create junction fixture");
        assert!(status.success(), "mklink /J failed: {status}");
        let report = grant_directory_tree_access(
            &root,
            DirectoryPrincipal::WellKnown(WellKnownDirectoryPrincipal::AllApplicationPackages),
            DirectoryTreeAccess::ReadExecute,
        )
        .expect("grant tree access");
        assert_eq!(report.entries_updated, 3);
        assert_eq!(report.link_like_entries_skipped, 1);
        std::fs::remove_dir(&junction).expect("remove junction");
        std::fs::remove_dir_all(&root).expect("remove root fixture");
        std::fs::remove_dir_all(&outside).expect("remove outside fixture");
    }
}
