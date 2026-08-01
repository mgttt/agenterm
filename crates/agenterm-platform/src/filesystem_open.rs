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

/// Opens an existing path without following a link-like final component.
///
/// The returned object is verified through the same opened handle. Intermediate
/// components are resolved by the host; callers that need component-wise
/// containment should retain a directory handle and use [`open_existing_child`].
pub fn open_existing(path: &Path, expected: ExistingEntryType) -> io::Result<File> {
    let file = crate::selected::filesystem_open::open_existing(path, expected)?;
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
    validate_child_name(name)?;
    let file = crate::selected::filesystem_open::open_existing_child(parent, name, expected)?;
    verify_opened_type(file, expected)
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
        assert_eq!(fs::read(outside.join("canary")).unwrap(), b"outside");

        drop(directory);
        fs::remove_dir(&junction).expect("remove junction");
        fs::remove_dir_all(base).expect("remove junction fixture");
    }
}
