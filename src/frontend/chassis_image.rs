//! Shared workbench loader for a pre-composed Chassis-L1/L2/L3 image.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug)]
pub(crate) struct LoadedChassisImage {
    pub(crate) root: PathBuf,
    pub(crate) native_loader: PathBuf,
    pub(crate) l3_name: String,
}

static LOADED_IMAGE: OnceLock<LoadedChassisImage> = OnceLock::new();

pub(crate) fn load_selected_image(
    root: Option<&Path>,
) -> Result<Option<&'static LoadedChassisImage>, String> {
    let Some(root) = root else {
        return Ok(None);
    };
    if let Some(loaded) = LOADED_IMAGE.get() {
        if loaded.root == root {
            return Ok(Some(loaded));
        }
        return Err("a different chassis image is already loaded".to_owned());
    }
    let loaded = load_image(root)?;
    LOADED_IMAGE
        .set(loaded)
        .map_err(|_| "chassis image initialization raced".to_owned())?;
    Ok(LOADED_IMAGE.get())
}

fn load_image(root: &Path) -> Result<LoadedChassisImage, String> {
    if !root.is_dir() {
        return Err(format!(
            "chassis image is not a directory: {}",
            root.display()
        ));
    }
    agenterm_chassis::check_product_image(root)
        .map_err(|error| format!("chassis image check failed: {error}"))?;
    let native_cell = agenterm_chassis::native_cell();
    if native_cell == "unknown" {
        return Err("this OS/ISA has no Chassis-L1 loader cell".to_owned());
    }
    let native_loader = root.join("l1").join(native_cell).join("loader");
    if !native_loader.is_file() {
        return Err(format!(
            "chassis image lacks native loader cell {native_cell}"
        ));
    }
    let app = agenterm_chassis::load_app(&root.join("l3/app.json"))
        .map_err(|error| format!("cannot load chassis L3 manifest: {error}"))?;
    Ok(LoadedChassisImage {
        root: root.to_path_buf(),
        native_loader,
        l3_name: app.name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agenterm_chassis::{CELLS, ProductManifest};
    use std::fs;

    fn write_image(root: &Path, l3_note: Option<&str>) {
        for cell in CELLS {
            let dir = root.join("l1").join(cell);
            fs::create_dir_all(&dir).expect("cell");
            fs::write(dir.join("loader"), format!("loader-{cell}")).expect("loader");
        }
        fs::create_dir_all(root.join("l2")).expect("l2");
        fs::write(
            root.join("l2/host-abi.json"),
            include_str!("../../crates/agenterm-chassis/l2/host-abi.json"),
        )
        .expect("abi");
        fs::create_dir_all(root.join("l3")).expect("l3");
        let note = l3_note.unwrap_or("");
        fs::write(
            root.join("l3/app.json"),
            format!(
                r#"{{"schema":1,"name":"workbench","capabilities":["tabs.list"],"note":"{note}"}}"#
            ),
        )
        .expect("app");
        let manifest = ProductManifest {
            schema: 1,
            compile: false,
            invokes_cargo: false,
            cells: CELLS.iter().map(|cell| (*cell).to_owned()).collect(),
            native_cell: None,
        };
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&manifest).expect("manifest json"),
        )
        .expect("manifest");
    }

    #[test]
    fn loads_native_cell_from_composed_image() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_image(tmp.path(), None);
        let image = load_image(tmp.path()).expect("load");
        assert!(image.native_loader.is_file());
        assert_eq!(image.l3_name, "workbench");
    }

    #[test]
    fn fails_closed_when_l3_names_native_library() {
        let tmp = tempfile::tempdir().expect("tmp");
        write_image(tmp.path(), Some("libc.so.6"));
        let error = load_image(tmp.path()).expect_err("forbidden L3");
        assert!(error.contains("libc.so.6"), "{error}");
    }
}
