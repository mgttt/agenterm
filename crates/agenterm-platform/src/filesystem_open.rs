//! Race-resistant opening of existing host filesystem objects.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    path::{Component, Path},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExistingEntryType {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ExistingEntryAccess {
    ReadOnly,
    SecurityDescriptor,
}

/// Opens an existing path without following a link-like final component.
///
/// The returned object is verified through the same opened handle. Intermediate
/// components are resolved by the host; callers that need component-wise
/// containment should retain a directory handle and use [`open_existing_child`].
pub fn open_existing(path: &Path, expected: ExistingEntryType) -> io::Result<File> {
    open_existing_with_access(path, expected, ExistingEntryAccess::ReadOnly)
}

pub fn open_existing_with_access(
    path: &Path,
    expected: ExistingEntryType,
    access: ExistingEntryAccess,
) -> io::Result<File> {
    let file = crate::selected::filesystem_open::open_existing(path, expected, access)?;
    verify_opened_type(file, expected)
}

/// Opens one existing child relative to an already-open directory object.
///
/// `name` must be exactly one ordinary component. The parent object, rather
/// than a reconstructed parent path, determines which directory is traversed.
pub fn open_existing_child(
    parent: &File,
    name: &OsStr,
    expected: ExistingEntryType,
) -> io::Result<File> {
    open_existing_child_with_access(parent, name, expected, ExistingEntryAccess::ReadOnly)
}

pub fn open_existing_child_with_access(
    parent: &File,
    name: &OsStr,
    expected: ExistingEntryType,
    access: ExistingEntryAccess,
) -> io::Result<File> {
    validate_child_name(name)?;
    let file =
        crate::selected::filesystem_open::open_existing_child(parent, name, expected, access)?;
    verify_opened_type(file, expected)
}

/// Opens an existing path one component at a time from its host root.
///
/// Every intermediate component is opened as a real directory through the
/// retained parent object, so a junction/symlink/reparse component cannot be
/// silently traversed. The final object is verified with the requested type.
pub fn open_existing_path(path: &Path, expected: ExistingEntryType) -> io::Result<File> {
    open_existing_path_with_access(path, expected, ExistingEntryAccess::ReadOnly)
}

pub fn open_existing_path_with_access(
    path: &Path,
    expected: ExistingEntryType,
    access: ExistingEntryAccess,
) -> io::Result<File> {
    let absolute = lexical_absolute(path)?;
    let (anchor, components) = split_root(&absolute)?;
    if components.is_empty() {
        return if expected == ExistingEntryType::Directory {
            open_existing_with_access(&anchor, ExistingEntryType::Directory, access)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "root path cannot be opened as a file",
            ))
        };
    }
    let mut current = open_existing_with_access(
        &anchor,
        ExistingEntryType::Directory,
        ExistingEntryAccess::ReadOnly,
    )?;
    let mut components = components.into_iter().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains a non-normal component after its root",
            ));
        };
        let is_final = components.peek().is_none();
        let child_type = if !is_final {
            ExistingEntryType::Directory
        } else {
            expected
        };
        let child_access = if is_final {
            access
        } else {
            ExistingEntryAccess::ReadOnly
        };
        current = open_existing_child_with_access(&current, name, child_type, child_access)?;
    }
    Ok(current)
}

fn lexical_absolute(path: &Path) -> io::Result<std::path::PathBuf> {
    let input = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    Ok(crate::filesystem::lexical_normalize(&input))
}

fn split_root(path: &Path) -> io::Result<(std::path::PathBuf, Vec<Component<'_>>)> {
    let mut components = path.components();
    let mut anchor = std::path::PathBuf::new();
    match components.next() {
        Some(Component::Prefix(prefix)) => {
            anchor.push(prefix.as_os_str());
            match components.next() {
                Some(Component::RootDir) => anchor.push(std::path::Path::new("\\")),
                Some(other) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unexpected root component: {other:?}"),
                    ));
                }
                None => {}
            }
        }
        Some(Component::RootDir) => {
            anchor.push(std::path::Path::new(std::path::MAIN_SEPARATOR_STR))
        }
        Some(other) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("absolute path has no root: {other:?}"),
            ));
        }
        None => return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty path")),
    }
    Ok((anchor, components.collect()))
}

fn validate_child_name(name: &OsStr) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if matches!(components.next(), Some(Component::Normal(component)) if component == name)
        && components.next().is_none()
    {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "child name must be one ordinary path component",
    ))
}

fn verify_opened_type(file: File, expected: ExistingEntryType) -> io::Result<File> {
    let facts = crate::filesystem_entry::opened_file_entry_facts(&file)?;
    let matches = match expected {
        ExistingEntryType::File => facts.is_real_file(),
        ExistingEntryType::Directory => facts.is_real_directory(),
    };
    if matches {
        Ok(file)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "opened filesystem object is link-like or has the wrong type",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Read as _, path::PathBuf};

    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agenterm-platform-open-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn opens_only_the_requested_existing_type() {
        let root = fixture("typed");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("create typed fixture");
        fs::write(root.join("file"), b"value").expect("write typed fixture");

        let directory = open_existing(&root, ExistingEntryType::Directory).expect("open root");
        let mut file = open_existing_child(&directory, OsStr::new("file"), ExistingEntryType::File)
            .expect("open child file");
        let mut contents = String::new();
        file.read_to_string(&mut contents).expect("read child file");
        assert_eq!(contents, "value");
        assert!(open_existing(&root, ExistingEntryType::File).is_err());
        assert!(
            open_existing_child(&directory, OsStr::new("file"), ExistingEntryType::Directory)
                .is_err()
        );

        drop(file);
        drop(directory);
        fs::remove_dir_all(root).expect("remove typed fixture");
    }

    #[test]
    fn opens_existing_path_componentwise() {
        let root = fixture("componentwise");
        let nested = root.join("nested");
        let file_path = nested.join("state");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&nested).expect("create componentwise fixture");
        fs::write(&file_path, b"componentwise").expect("write componentwise fixture");

        let mut file = open_existing_path(&file_path, ExistingEntryType::File)
            .expect("open componentwise file");
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .expect("read componentwise file");
        assert_eq!(contents, "componentwise");
        drop(file);
        fs::remove_dir_all(root).expect("remove componentwise fixture");
    }

    #[test]
    fn lexical_absolute_does_not_pop_past_the_host_root() {
        let current = std::env::current_dir().expect("read current directory");
        let (anchor, _) = split_root(&current).expect("split current directory root");
        let escaped = anchor.join("..").join("componentwise-root");
        let normalized = lexical_absolute(&escaped).expect("normalize escaped root path");
        assert_eq!(normalized, anchor.join("componentwise-root"));
    }

    #[test]
    fn rejects_non_component_child_names_before_native_open() {
        let root = fixture("components");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("create component fixture");
        let directory = open_existing(&root, ExistingEntryType::Directory).expect("open root");

        for name in ["", ".", "..", "nested/child"] {
            let error = open_existing_child(&directory, OsStr::new(name), ExistingEntryType::File)
                .expect_err("invalid child component");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "name={name:?}");
        }

        drop(directory);
        fs::remove_dir_all(root).expect("remove component fixture");
    }

    #[test]
    fn retained_parent_identity_survives_path_replacement() {
        let base = fixture("identity");
        let original = base.join("root");
        let retained = base.join("retained");
        let replacement = base.join("replacement");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&original).expect("create original root");
        fs::create_dir(&replacement).expect("create replacement root");
        fs::write(original.join("trusted"), b"trusted").expect("write trusted child");
        fs::write(replacement.join("attacker"), b"attacker").expect("write attacker child");

        let directory =
            open_existing(&original, ExistingEntryType::Directory).expect("retain original root");
        fs::rename(&original, &retained).expect("rename retained root");
        fs::rename(&replacement, &original).expect("install replacement root");

        let mut trusted =
            open_existing_child(&directory, OsStr::new("trusted"), ExistingEntryType::File)
                .expect("open child through retained identity");
        let mut contents = String::new();
        trusted
            .read_to_string(&mut contents)
            .expect("read trusted child");
        assert_eq!(contents, "trusted");
        assert!(
            open_existing_child(&directory, OsStr::new("attacker"), ExistingEntryType::File)
                .is_err()
        );

        drop(trusted);
        drop(directory);
        fs::remove_dir_all(base).expect("remove identity fixture");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_link_children() {
        use std::os::unix::fs::symlink;

        let base = fixture("unix-link");
        let root = base.join("root");
        let outside = base.join("outside");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&root).expect("create link root");
        fs::create_dir(&outside).expect("create link target");
        fs::write(outside.join("canary"), b"outside").expect("write outside canary");
        symlink(&outside, root.join("link")).expect("create directory symlink");

        let directory = open_existing(&root, ExistingEntryType::Directory).expect("open root");
        assert!(
            open_existing_child(&directory, OsStr::new("link"), ExistingEntryType::Directory)
                .is_err()
        );
        assert_eq!(fs::read(outside.join("canary")).unwrap(), b"outside");

        drop(directory);
        fs::remove_dir_all(base).expect("remove link fixture");
    }

    #[cfg(windows)]
    #[test]
    fn rejects_junction_children() {
        let base = fixture("windows-junction");
        let root = base.join("root");
        let outside = base.join("outside");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&root).expect("create junction root");
        fs::create_dir(&outside).expect("create junction target");
        fs::write(outside.join("canary"), b"outside").expect("write outside canary");
        fs::create_dir(outside.join("nested")).expect("create nested junction target");
        let junction = root.join("junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("run mklink junction fixture");
        assert!(status.success(), "mklink /J fixture failed: {status}");

        let directory = open_existing(&root, ExistingEntryType::Directory).expect("open root");
        assert!(
            open_existing_child(
                &directory,
                OsStr::new("junction"),
                ExistingEntryType::Directory
            )
            .is_err()
        );
        assert!(
            open_existing_path(&junction.join("nested"), ExistingEntryType::Directory).is_err()
        );
        assert_eq!(fs::read(outside.join("canary")).unwrap(), b"outside");

        drop(directory);
        fs::remove_dir(&junction).expect("remove junction");
        fs::remove_dir_all(base).expect("remove junction fixture");
    }
}
