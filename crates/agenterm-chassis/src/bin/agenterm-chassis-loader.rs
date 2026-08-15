use std::path::PathBuf;
use std::process::ExitCode;

#[path = "../loader/mod.rs"]
mod loader;

const EXIT_USAGE: u8 = 2;
const EXIT_IMAGE_REJECTED: u8 = 3;
const EXIT_PRESENT_FAILED: u8 = 4;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next().map(PathBuf::from) else {
        eprintln!("usage: agenterm-chassis-loader IMAGE_DIR");
        return ExitCode::from(EXIT_USAGE);
    };
    if args.next().is_some() {
        eprintln!("usage: agenterm-chassis-loader IMAGE_DIR");
        return ExitCode::from(EXIT_USAGE);
    }

    if let Err(error) = check_native_cell(&root) {
        eprintln!("agenterm-chassis-loader: {error}");
        return ExitCode::from(EXIT_IMAGE_REJECTED);
    }

    ExitCode::from(exit_status(loader::load_then(&root, loader::present_image)))
}

fn exit_status<E: std::fmt::Display>(result: Result<(), loader::LoadThenError<E>>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(loader::LoadThenError::Image(error)) => {
            eprintln!("agenterm-chassis-loader: {error}");
            EXIT_IMAGE_REJECTED
        }
        Err(loader::LoadThenError::Present(error)) => {
            eprintln!("agenterm-chassis-loader: native presentation failed: {error}");
            EXIT_PRESENT_FAILED
        }
    }
}

fn check_native_cell(root: &std::path::Path) -> Result<(), String> {
    let path = root.join("manifest.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("image rejected: cannot read product manifest: {error}"))?;
    let manifest: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("image rejected: invalid product manifest: {error}"))?;
    let Some(native_cell) = manifest.get("native_cell") else {
        return Err("image rejected: product manifest must declare native_cell".to_string());
    };
    if native_cell.is_null() {
        return Ok(());
    }
    let declared = native_cell.as_str().ok_or_else(|| {
        "image rejected: product manifest native_cell must be a string or null".to_string()
    })?;
    let actual = agenterm_chassis::native_cell();
    if actual == "unknown" || declared != actual {
        return Err(format!(
            "image rejected: native_cell `{declared}` does not match loader cell `{actual}`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EXIT_PRESENT_FAILED, exit_status, loader};

    #[test]
    fn native_presenter_failure_has_distinct_nonzero_exit_status() {
        let result: Result<(), loader::LoadThenError<&str>> =
            Err(loader::LoadThenError::Present("present failed"));
        assert_eq!(exit_status(result), EXIT_PRESENT_FAILED);
    }

    #[test]
    fn successful_native_presenter_maps_to_zero() {
        let result: Result<(), loader::LoadThenError<&str>> = Ok(());
        assert_eq!(exit_status(result), 0);
    }
}
