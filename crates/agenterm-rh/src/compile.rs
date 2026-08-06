use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::{transpile::transpile_cdylib, RhError, RH_VERSION};

const GENERATED_CRATE: &str = "rh_pack_generated";

pub struct CompileOutput {
    pub native_path: PathBuf,
    pub manifest_path: PathBuf,
    pub source_hash: String,
    pub native_hash: String,
}

pub fn compile_native(source: &str, output_path: &Path) -> Result<CompileOutput, RhError> {
    let rust = transpile_cdylib(source)?;
    let source_hash = hash_bytes(source.as_bytes());

    let scratch = tempfile::tempdir().map_err(|err| RhError::Compile(err.to_string()))?;
    let crate_root = scratch.path().join("crate");
    std::fs::create_dir_all(crate_root.join("src"))
        .map_err(|err| RhError::Compile(err.to_string()))?;
    std::fs::write(crate_root.join("Cargo.toml"), generated_cargo_toml())
        .map_err(|err| RhError::Compile(err.to_string()))?;
    std::fs::write(crate_root.join("src/lib.rs"), rust)
        .map_err(|err| RhError::Compile(err.to_string()))?;

    let target_dir = scratch.path().join("target");
    std::fs::write(
        crate_root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.97.0\"\n",
    )
    .map_err(|err| RhError::Compile(err.to_string()))?;

    let status = cargo_command()
        .arg("build")
        .arg("--release")
        .arg("--target-dir")
        .arg(&target_dir)
        .current_dir(&crate_root)
        .status()
        .map_err(|err| RhError::Compile(format!("failed to spawn cargo: {err}")))?;
    if !status.success() {
        return Err(RhError::Compile(format!(
            "cargo build failed with status {status}"
        )));
    }

    let extension = native_extension();
    let artifact = target_dir
        .join("release")
        .join(format!("lib{GENERATED_CRATE}.{extension}"));
    if !artifact.is_file() {
        return Err(RhError::Compile(format!(
            "expected native artifact at {}",
            artifact.display()
        )));
    }

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| RhError::Compile(err.to_string()))?;
        }
    }
    std::fs::copy(&artifact, output_path).map_err(|err| RhError::Compile(err.to_string()))?;

    let native_hash = hash_file(output_path)?;
    let manifest_path = manifest_path_for(output_path);
    write_manifest(
        &manifest_path,
        source_hash.as_str(),
        native_hash.as_str(),
        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rh_pack.so"),
    )?;

    Ok(CompileOutput {
        native_path: output_path.to_path_buf(),
        manifest_path,
        source_hash,
        native_hash,
    })
}

pub fn hash_file(path: &Path) -> Result<String, RhError> {
    let bytes = std::fs::read(path).map_err(|err| RhError::Compile(err.to_string()))?;
    Ok(hash_bytes(&bytes))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_manifest(
    path: &Path,
    source_hash: &str,
    native_hash: &str,
    native_file: &str,
) -> Result<(), RhError> {
    let json = format!(
        "{{\n  \"schema\": \"agenterm.rh-pack-manifest/v1\",\n  \
         \"rh_version\": \"{RH_VERSION}\",\n  \
         \"source_hash\": \"{source_hash}\",\n  \
         \"native_hash\": \"{native_hash}\",\n  \
         \"native_file\": \"{native_file}\",\n  \
         \"entry_symbol\": \"rh_entry\"\n}}\n"
    );
    std::fs::write(path, json).map_err(|err| RhError::Compile(err.to_string()))
}

fn manifest_path_for(native_path: &Path) -> PathBuf {
    native_path.with_extension("manifest.json")
}

fn cargo_command() -> Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| {
        std::env::var("RUSTC")
            .ok()
            .and_then(|rustc| {
                PathBuf::from(rustc)
                    .parent()
                    .map(|dir| dir.join("cargo"))
            })
            .and_then(|path| path.to_str().map(str::to_owned))
            .unwrap_or_else(|| "cargo".to_string())
    });
    let mut command = Command::new(cargo);
    command.stdin(Stdio::null());
    command
}

fn generated_cargo_toml() -> String {
    format!(
        "[package]\n\
         name = \"{GENERATED_CRATE}\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         publish = false\n\n\
         [lib]\n\
         crate-type = [\"cdylib\"]\n\n\
         [dependencies]\n\
         rhai = {{ version = \"1.22\", default-features = false, features = [\"std\"] }}\n"
    )
}

pub fn native_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

#[cfg(test)]
mod tests {
    use super::{compile_native, hash_bytes};

    #[test]
    fn source_hash_is_stable() {
        assert_eq!(
            hash_bytes(b"fn add(a, b) { a + b }"),
            hash_bytes(b"fn add(a, b) { a + b }")
        );
    }

    #[test]
    fn compiles_simple_pack_to_native() {
        let out = std::env::temp_dir().join(format!(
            "agenterm-rh-test-{}.{}",
            std::process::id(),
            super::native_extension()
        ));
        let _ = std::fs::remove_file(&out);
        let manifest = out.with_extension("manifest.json");
        let _ = std::fs::remove_file(&manifest);

        let output = compile_native("fn add(a, b) { a + b }", &out).expect("compile");
        assert!(out.is_file());
        assert!(output.manifest_path.is_file());
        assert_eq!(output.native_hash, super::hash_file(&out).expect("hash"));
        assert_eq!(output.native_path, out);
    }
}
