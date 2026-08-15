use std::path::PathBuf;
use std::process::ExitCode;

#[path = "../loader/mod.rs"]
mod loader;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next().map(PathBuf::from) else {
        eprintln!("usage: agenterm-chassis-loader IMAGE_DIR");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: agenterm-chassis-loader IMAGE_DIR");
        return ExitCode::from(2);
    }

    match loader::load_then(&root, loader::present_image) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("agenterm-chassis-loader: {error}");
            ExitCode::from(1)
        }
    }
}
