use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use agenterm_chassis::bytecode::{L2Source, Program, assemble};
use agenterm_chassis::l2_dispatch::{Dispatcher, HostCallback};
use agenterm_chassis::vm::{DEFAULT_MAX_STEPS, run};
use agenterm_chassis::{ChassisError, check_product_image, inspect, load_app};

#[cfg(feature = "loader")]
mod native;

/// A composed image that passed the complete L1/L2/L3 layout check.
///
/// Construction is private so a presenter cannot accidentally receive an
/// unchecked image.
#[derive(Debug)]
pub struct LoadedImage {
    report: serde_json::Value,
    #[allow(dead_code)]
    root: PathBuf,
    #[allow(dead_code)]
    host_abi: String,
    #[allow(dead_code)]
    app_manifest: String,
    #[allow(dead_code)]
    declared_capabilities: Vec<String>,
    #[allow(dead_code)]
    programs: BTreeMap<String, LoadedProgram>,
}

#[derive(Debug)]
struct LoadedProgram {
    program: Program,
    source_ir: L2Source,
    source: Vec<u8>,
}

impl LoadedImage {
    pub fn l3_name(&self) -> &str {
        self.report["l3_name"].as_str().unwrap_or("unnamed")
    }

    pub fn capability_count(&self) -> usize {
        self.report["l3_capabilities"]
            .as_array()
            .map_or(0, Vec::len)
    }

    /// Run one checked L2 program through Host ABI v3 with the default VM budget.
    #[allow(dead_code)]
    pub fn eval_l2<H: HostCallback>(
        &self,
        program_name: &str,
        host: H,
    ) -> Result<(i64, H), L2EvalError> {
        self.eval_l2_bounded(program_name, host, DEFAULT_MAX_STEPS)
    }

    /// Run one checked L2 program through Host ABI v3 with an explicit VM budget.
    #[allow(dead_code)]
    pub fn eval_l2_bounded<H: HostCallback>(
        &self,
        program_name: &str,
        host: H,
        max_steps: u32,
    ) -> Result<(i64, H), L2EvalError> {
        let loaded_program = self
            .programs
            .get(program_name)
            .ok_or_else(|| L2EvalError::UnknownProgram(program_name.to_string()))?;
        self.verify_dispatch_inputs(program_name, loaded_program)?;
        assemble(&loaded_program.source_ir, Some(&self.declared_capabilities))
            .map_err(L2EvalError::Dispatch)?;
        let mut dispatcher =
            Dispatcher::from_host_abi_json(&self.host_abi, &self.declared_capabilities, host)
                .map_err(|error| L2EvalError::Dispatch(error.to_string()))?;
        let value =
            run(&loaded_program.program, &mut dispatcher, max_steps).map_err(L2EvalError::Vm)?;
        Ok((value, dispatcher.into_host()))
    }

    fn verify_dispatch_inputs(
        &self,
        program_name: &str,
        loaded_program: &LoadedProgram,
    ) -> Result<(), L2EvalError> {
        let inputs = [
            (
                "L2 Host ABI",
                self.root.join("l2/host-abi.json"),
                self.host_abi.as_bytes(),
            ),
            (
                "L3 manifest",
                self.root.join("l3/app.json"),
                self.app_manifest.as_bytes(),
            ),
            (
                "L2 program",
                self.root.join("l2/programs").join(program_name),
                loaded_program.source.as_slice(),
            ),
        ];
        for (label, path, expected) in inputs {
            let actual = fs::read(path)
                .map_err(|error| L2EvalError::Tampered(format!("{label} unreadable: {error}")))?;
            if actual != expected {
                return Err(L2EvalError::Tampered(format!(
                    "{label} changed after image load"
                )));
            }
        }
        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum L2EvalError {
    UnknownProgram(String),
    Tampered(String),
    Dispatch(String),
    Vm(String),
}

impl std::fmt::Display for L2EvalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProgram(name) => write!(formatter, "unknown L2 program `{name}`"),
            Self::Tampered(reason) => write!(formatter, "composed image tampered: {reason}"),
            Self::Dispatch(reason) => write!(formatter, "L2 dispatch rejected: {reason}"),
            Self::Vm(reason) => write!(formatter, "L2 VM rejected: {reason}"),
        }
    }
}

impl std::error::Error for L2EvalError {}

/// Load and inspect an unpacked composed image without opening a window.
///
/// `check_layout` rejects undeclared capabilities and native doors in L3.
/// Only after that check succeeds is the inspected image wrapped in the
/// unforgeable [`LoadedImage`] state accepted by native presentation.
pub fn load_image(root: &Path) -> Result<LoadedImage, ChassisError> {
    check_product_image(root)?;
    let report = inspect(root)?;
    let host_abi = fs::read_to_string(root.join("l2/host-abi.json"))?;
    let app_manifest = fs::read_to_string(root.join("l3/app.json"))?;
    let declared_capabilities = load_app(&root.join("l3/app.json"))?.capabilities;
    Dispatcher::from_host_abi_json(&host_abi, &declared_capabilities, ValidateOnlyHost)
        .map_err(|error| ChassisError::Check(error.to_string()))?;
    let programs = load_programs(root)?;
    Ok(LoadedImage {
        report,
        root: root.to_path_buf(),
        host_abi,
        app_manifest,
        declared_capabilities,
        programs,
    })
}

fn load_programs(root: &Path) -> Result<BTreeMap<String, LoadedProgram>, ChassisError> {
    let programs_root = root.join("l2/programs");
    let mut programs = BTreeMap::new();
    if !programs_root.is_dir() {
        return Ok(programs);
    }
    for entry in fs::read_dir(&programs_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            return Err(ChassisError::Check(format!(
                "L2 programs entry {} is not a file",
                entry.path().display()
            )));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| ChassisError::Check("L2 program name is not UTF-8".to_string()))?;
        if !name.ends_with(".json") {
            return Err(ChassisError::Check(format!(
                "L2 program `{name}` must be JSON"
            )));
        }
        let source_bytes = fs::read(entry.path())?;
        let source_text = std::str::from_utf8(&source_bytes).map_err(|error| {
            ChassisError::Check(format!("invalid L2 program `{name}` UTF-8: {error}"))
        })?;
        let source = L2Source::from_json(source_text).map_err(|error| {
            ChassisError::Check(format!("invalid L2 program `{name}`: {error}"))
        })?;
        let program = assemble(&source, None).map_err(|error| {
            ChassisError::Check(format!("invalid L2 program `{name}`: {error}"))
        })?;
        if programs
            .insert(
                name.clone(),
                LoadedProgram {
                    program,
                    source_ir: source,
                    source: source_bytes,
                },
            )
            .is_some()
        {
            return Err(ChassisError::Check(format!(
                "duplicate L2 program `{name}`"
            )));
        }
    }
    Ok(programs)
}

struct ValidateOnlyHost;

impl HostCallback for ValidateOnlyHost {
    fn call(
        &mut self,
        _capability: &str,
        _parameters: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("validation host cannot be called".to_string())
    }
}

#[derive(Debug)]
pub enum LoadThenError<E> {
    Image(ChassisError),
    Present(E),
}

impl<E: std::fmt::Display> std::fmt::Display for LoadThenError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(error) => write!(formatter, "image rejected: {error}"),
            Self::Present(error) => write!(formatter, "image presentation failed: {error}"),
        }
    }
}

impl<E> std::error::Error for LoadThenError<E> where E: std::error::Error + 'static {}

/// Enforce load-before-present ordering while allowing headless verification.
pub fn load_then<T, E>(
    root: &Path,
    present: impl FnOnce(LoadedImage) -> Result<T, E>,
) -> Result<T, LoadThenError<E>> {
    let image = load_image(root).map_err(LoadThenError::Image)?;
    present(image).map_err(LoadThenError::Present)
}

#[cfg(feature = "loader")]
pub fn present_image(
    image: LoadedImage,
) -> Result<(), agenterm_platform::window_host::PixelWindowError> {
    native::present(image)
}
