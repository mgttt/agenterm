use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use agenterm_chassis::{check_layout, compose, inspect, native_cell};

fn main() -> ExitCode {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!(
            "usage:\n  agenterm-chassis native-cell\n  agenterm-chassis compose --from DIR --out DIR\n  agenterm-chassis check DIR\n  agenterm-chassis inspect DIR"
        );
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    let result = match cmd.as_str() {
        "native-cell" => {
            println!("{}", native_cell());
            Ok(())
        }
        "compose" => {
            let from = take_opt(&args, "--from");
            let out = take_opt(&args, "--out");
            match (from, out) {
                (Some(from), Some(out)) => compose(&from, &out).map(|manifest| {
                    println!("{}", serde_json::to_string_pretty(&manifest).expect("json"));
                }),
                _ => Err(agenterm_chassis::ChassisError::Usage(
                    "compose requires --from DIR --out DIR".into(),
                )),
            }
        }
        "check" => {
            let dir = args.first().map(PathBuf::from);
            match dir {
                Some(dir) => check_layout(&dir).map(|()| println!("ok")),
                None => Err(agenterm_chassis::ChassisError::Usage(
                    "check requires DIR".into(),
                )),
            }
        }
        "inspect" => {
            let dir = args.first().map(PathBuf::from);
            match dir {
                Some(dir) => inspect(&dir).map(|value| {
                    println!("{}", serde_json::to_string_pretty(&value).expect("json"));
                }),
                None => Err(agenterm_chassis::ChassisError::Usage(
                    "inspect requires DIR".into(),
                )),
            }
        }
        other => Err(agenterm_chassis::ChassisError::Usage(format!(
            "unknown command {other}"
        ))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn take_opt(args: &[String], name: &str) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| PathBuf::from(&pair[1]))
}
