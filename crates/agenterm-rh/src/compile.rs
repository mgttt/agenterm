use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

use crate::{
    RhError,
    manifest::{RhPackManifest, manifest_path_for},
    transpile::{cc_line_count, transpile_cdylib},
};

const GENERATED_CRATE: &str = "rh_pack_generated";

pub struct CompileOutput {
    pub native_path: PathBuf,
    pub manifest_path: PathBuf,
    pub source_hash: String,
    pub native_hash: String,
}

pub fn compile_native(source: &str, output_path: &Path) -> Result<CompileOutput, RhError> {
    compile_native_for_target(source, output_path, None)
}

pub fn compile_native_for_target(
    source: &str,
    output_path: &Path,
    target: Option<&str>,
) -> Result<CompileOutput, RhError> {
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

    let mut command = cargo_command();
    command
        .arg("build")
        .arg("--release")
        .arg("--jobs")
        .arg(aot_build_jobs().to_string())
        .arg("--target-dir")
        .arg(&target_dir);
    if let Some(triple) = target {
        command.arg("--target").arg(triple);
    }
    let status = command
        .current_dir(&crate_root)
        .status()
        .map_err(|err| RhError::Compile(format!("failed to spawn cargo: {err}")))?;
    if !status.success() {
        return Err(RhError::Compile(format!(
            "cargo build failed with status {status}"
        )));
    }

    let extension = native_extension_for_target(target);
    let release_dir = match target {
        Some(triple) => target_dir.join(triple).join("release"),
        None => target_dir.join("release"),
    };
    // Windows MSVC emits `{crate}.dll`; Unix (and some GNU toolchains) use
    // the `lib` prefix. Accept either so host packs are portable.
    let artifact_candidates = [
        release_dir.join(format!("lib{GENERATED_CRATE}.{extension}")),
        release_dir.join(format!("{GENERATED_CRATE}.{extension}")),
    ];
    let artifact = artifact_candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            RhError::Compile(format!(
                "expected native artifact at {} or {}",
                artifact_candidates[0].display(),
                artifact_candidates[1].display()
            ))
        })?;

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| RhError::Compile(err.to_string()))?;
    }
    std::fs::copy(&artifact, output_path).map_err(|err| RhError::Compile(err.to_string()))?;

    let native_hash = hash_file(output_path)?;
    let manifest_path = manifest_path_for(output_path);
    RhPackManifest::write(
        &manifest_path,
        source_hash.as_str(),
        native_hash.as_str(),
        output_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("rh_pack.so"),
        cc_line_count(source),
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

/// Parallelism for one generated-crate build.
///
/// Every AOT compile builds a fresh crate in its own temp target dir, so
/// nothing is shared between them and each one defaults to a rustc per core.
/// A test suite compiling several sources at once therefore multiplies into
/// cores × concurrent-compiles, which on an 8-core / 8 GB machine exhausts CPU
/// and memory and stalls the host rather than merely running slowly. Observed
/// directly: five concurrent release builds of the old rhai-dependent
/// generated crate saturated the machine.
///
/// 822befd dropped `rhai` from the generated crate, so each build is far
/// lighter now, but the multiplication is structural and returns with any
/// heavier dependency. Two jobs keeps a single compile responsive while
/// leaving room for its siblings and the editor; `AGENTERM_RH_BUILD_JOBS`
/// overrides it for hosts that can afford more.
fn aot_build_jobs() -> usize {
    std::env::var("AGENTERM_RH_BUILD_JOBS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|jobs| *jobs > 0)
        .unwrap_or(2)
}

fn cargo_command() -> Command {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| {
        std::env::var("RUSTC")
            .ok()
            .and_then(|rustc| PathBuf::from(rustc).parent().map(|dir| dir.join("cargo")))
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
         serde_json = \"1\"\n\
         sha2 = \"0.10\"\n"
    )
}

pub fn native_extension() -> &'static str {
    native_extension_for_target(None)
}

pub fn native_extension_for_target(target: Option<&str>) -> &'static str {
    let triple = target.unwrap_or("");
    if triple.contains("windows") || (target.is_none() && cfg!(target_os = "windows")) {
        "dll"
    } else if triple.contains("apple")
        || triple.contains("darwin")
        || (target.is_none() && cfg!(target_os = "macos"))
    {
        "dylib"
    } else {
        "so"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile_native, compile_native_for_target, generated_cargo_toml, hash_bytes,
        native_extension_for_target,
    };

    #[test]
    fn generated_native_pack_has_no_rhai_runtime_dependency() {
        let manifest = generated_cargo_toml();
        assert!(manifest.contains("serde_json = \"1\""));
        assert!(manifest.contains("sha2 = \"0.10\""));
        assert!(!manifest.contains("rhai"));
    }

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

    #[test]
    fn cross_compiles_reference_pack_when_target_env_set() {
        let Some(target) = std::env::var("AGENTERM_RH_QUALIFY_TARGET")
            .ok()
            .filter(|value| !value.is_empty())
        else {
            return;
        };
        let out = std::env::temp_dir().join(format!(
            "agenterm-rh-cross-{}.{}",
            std::process::id(),
            native_extension_for_target(Some(target.as_str()))
        ));
        let _ = std::fs::remove_file(&out);
        let output = compile_native_for_target("fn entry() { 11 }", &out, Some(target.as_str()))
            .expect("cross compile");
        assert!(out.is_file());
        assert_eq!(output.native_hash, super::hash_file(&out).expect("hash"));
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(output.manifest_path);
    }
}
