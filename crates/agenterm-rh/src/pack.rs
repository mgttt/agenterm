use std::path::{Path, PathBuf};

use crate::{
    RhError,
    compile::compile_native,
    load::{RhNativeModule, verify_native_hash},
    manifest::{RhPackManifest, default_pack_layout, manifest_path_for},
};

pub struct RhPack {
    pub root: PathBuf,
    pub manifest: RhPackManifest,
    pub native_path: PathBuf,
    module: RhNativeModule,
}

impl RhPack {
    pub fn load(path: &Path) -> Result<Self, RhError> {
        let (root, manifest_path, native_path) = resolve_pack_paths(path)?;
        let manifest = RhPackManifest::read(&manifest_path)?;
        verify_native_hash(&native_path, &manifest.native_hash)?;
        let module = RhNativeModule::load(&native_path)?;
        Ok(Self {
            root,
            manifest,
            native_path,
            module,
        })
    }

    pub fn entry_value(&self) -> i64 {
        self.module.call_entry()
    }

    pub fn cc_lines(&self) -> Vec<String> {
        self.module.cc_lines()
    }

    /// See `RhNativeModule::register_stub_host`. Qualification runs a pack with
    /// no host, so it opts into the stub callbacks explicitly.
    pub fn register_stub_host(&self) -> Result<(), RhError> {
        self.module.register_stub_host()
    }

    pub fn api_version(&self) -> u32 {
        self.module.api_version()
    }
}

pub struct PackBuildOutput {
    pub root: PathBuf,
    pub native_path: PathBuf,
    pub manifest_path: PathBuf,
    pub source_path: PathBuf,
    pub compile: crate::CompileOutput,
}

pub fn build_pack_dir(source: &str, dir: &Path) -> Result<PackBuildOutput, RhError> {
    std::fs::create_dir_all(dir).map_err(|err| RhError::Compile(err.to_string()))?;
    let (native_path, _manifest_scratch) = default_pack_layout(dir);
    let compile = compile_native(source, &native_path)?;
    let source_path = dir.join("entry.rh");
    std::fs::write(&source_path, source).map_err(|err| RhError::Compile(err.to_string()))?;
    let manifest_path = dir.join("manifest.json");
    RhPackManifest::write(
        &manifest_path,
        compile.source_hash.as_str(),
        compile.native_hash.as_str(),
        native_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pack.so"),
        crate::transpile::cc_line_count(source),
    )?;
    let _ = std::fs::remove_file(&compile.manifest_path);
    Ok(PackBuildOutput {
        root: dir.to_path_buf(),
        native_path,
        manifest_path,
        source_path,
        compile,
    })
}

fn resolve_pack_paths(path: &Path) -> Result<(PathBuf, PathBuf, PathBuf), RhError> {
    if path.is_dir() {
        let manifest_path = path.join("manifest.json");
        let manifest = RhPackManifest::read(&manifest_path)?;
        let native_path = path.join(&manifest.native_file);
        return Ok((path.to_path_buf(), manifest_path, native_path));
    }

    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if matches!(extension, "so" | "dylib" | "dll") {
        let manifest_path = manifest_path_for(path);
        if !manifest_path.is_file() {
            let sibling = path.with_file_name("manifest.json");
            if sibling.is_file() {
                let _manifest = RhPackManifest::read(&sibling)?;
                return Ok((
                    path.parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| PathBuf::from(".")),
                    sibling,
                    path.to_path_buf(),
                ));
            }
            return Err(RhError::Compile(format!(
                "missing manifest for native pack: {}",
                manifest_path.display()
            )));
        }
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        return Ok((root, manifest_path, path.to_path_buf()));
    }

    Err(RhError::Compile(format!(
        "unsupported rh pack path (expected directory or native library): {}",
        path.display()
    )))
}
