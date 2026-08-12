//! Windows atomic file publication adapter.

use std::{
    ffi::OsString,
    os::windows::ffi::{OsStrExt as _, OsStringExt as _},
    path::{Path, PathBuf},
    ptr::null_mut,
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION},
    Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, GetFileAttributesW, GetFullPathNameW, INVALID_FILE_ATTRIBUTES,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    },
};

pub fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    // Verbatim spelling only — the canonicalize() this replaced also resolved
    // symlinks, which owned-sibling publication deliberately no longer pays for.
    // What it must not drop is MAX_PATH escape: MoveFileExW on a plain 280-byte
    // path fails ERROR_PATH_NOT_FOUND.
    let source = verbatim(full_path(source)?);
    let destination = verbatim(full_path(destination)?);
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

pub fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    // MOVEFILE_WRITE_THROUGH owns the Windows durability barrier.
    Ok(())
}

pub fn normalize_owned_destination(destination: &Path) -> std::io::Result<PathBuf> {
    let destination = verbatim(full_path(destination)?);
    let parent = destination
        .parent()
        .ok_or_else(|| std::io::Error::other("destination parent required"))?;
    let parent = parent
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let attributes = unsafe {
        // SAFETY: parent is a live NUL-terminated UTF-16 path.
        GetFileAttributesW(parent.as_ptr())
    };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(std::io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "destination parent is not a directory",
        ));
    }
    Ok(destination)
}

/// Re-attach the verbatim prefix that `fs::canonicalize` used to supply.
///
/// `GetFullPathNameW` normalizes but returns a *plain* Win32 path, and plain
/// paths stay bound by MAX_PATH — every subsequent `GetFileAttributesW` /
/// `CreateFileW` / `MoveFileExW` on a deep destination would fail with
/// ERROR_PATH_NOT_FOUND. Prefixing is safe precisely because the path is
/// already fully normalized: verbatim only disables the normalization we just
/// performed ourselves.
fn verbatim(path: PathBuf) -> PathBuf {
    use std::path::{Component, Prefix};

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return path;
    };
    // \\server\share\rest keeps its two leading separators only in the plain
    // form; the verbatim spelling replaces them with the UNC\ marker.
    let (skip, lead) = match prefix.kind() {
        Prefix::Disk(_) => (0_usize, r"\\?\"),
        Prefix::UNC(..) => (2_usize, r"\\?\UNC\"),
        _ => return path,
    };
    let mut units: Vec<u16> = lead.encode_utf16().collect();
    units.extend(path.as_os_str().encode_wide().skip(skip));
    PathBuf::from(OsString::from_wide(&units))
}

fn full_path(path: &Path) -> std::io::Result<PathBuf> {
    const INITIAL_UNITS: usize = 512;
    const MAX_UNITS: usize = 32_768;
    let input = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut output = vec![0_u16; INITIAL_UNITS];
    loop {
        let length = unsafe {
            // SAFETY: input is NUL-terminated; output is initialized writable
            // storage and no file-part pointer is requested.
            GetFullPathNameW(
                input.as_ptr(),
                output.len() as u32,
                output.as_mut_ptr(),
                null_mut(),
            )
        } as usize;
        if length == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if length < output.len() {
            output.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&output)));
        }
        let capacity = (length + 1).max(output.len().saturating_mul(2));
        if capacity > MAX_UNITS {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "absolute destination path exceeds the Windows path bound",
            ));
        }
        output.resize(capacity, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefixes_only_plain_win32_spellings() {
        assert_eq!(
            verbatim(PathBuf::from(r"C:\publish\manifest.json")),
            PathBuf::from(r"\\?\C:\publish\manifest.json")
        );
        assert_eq!(
            verbatim(PathBuf::from(r"\\host\share\manifest.json")),
            PathBuf::from(r"\\?\UNC\host\share\manifest.json")
        );
        // Already verbatim, and device namespaces, must pass through untouched.
        assert_eq!(
            verbatim(PathBuf::from(r"\\?\C:\publish\manifest.json")),
            PathBuf::from(r"\\?\C:\publish\manifest.json")
        );
        assert_eq!(
            verbatim(PathBuf::from(r"\\?\UNC\host\share\manifest.json")),
            PathBuf::from(r"\\?\UNC\host\share\manifest.json")
        );
    }

    #[test]
    fn normalized_destinations_survive_beyond_max_path() {
        let mut deep =
            std::env::temp_dir().join(format!("agenterm-publish-long-{}", std::process::id()));
        while deep.as_os_str().len() < 280 {
            deep.push("qualified-package-boundary-segment");
        }
        std::fs::create_dir_all(&deep).expect("deep parent");
        let normalized = normalize_owned_destination(&deep.join("manifest.json"))
            .expect("a deep destination must normalize");
        assert!(
            normalized
                .as_os_str()
                .to_string_lossy()
                .starts_with(r"\\?\"),
            "deep destinations must be handed on in verbatim form: {normalized:?}"
        );
        std::fs::remove_dir_all(
            std::env::temp_dir().join(format!("agenterm-publish-long-{}", std::process::id())),
        )
        .expect("cleanup");
    }
}
