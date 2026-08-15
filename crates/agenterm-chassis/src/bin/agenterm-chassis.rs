use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use agenterm_chassis::bytecode::{L2Source, assemble};
use agenterm_chassis::vm::{CapHost, DEFAULT_MAX_STEPS, run};
use agenterm_chassis::{check_layout, compose, inspect, native_cell};

fn main() -> ExitCode {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!(
            "usage:\n  agenterm-chassis native-cell\n  agenterm-chassis compose --from DIR --out DIR\n  agenterm-chassis check DIR\n  agenterm-chassis inspect DIR\n  agenterm-chassis eval-l2 FILE.json"
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
        "eval-l2" => {
            let file = args.first().map(PathBuf::from);
            match file {
                Some(file) => eval_l2(&file),
                None => Err(agenterm_chassis::ChassisError::Usage(
                    "eval-l2 requires FILE.json".into(),
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

struct CountingHost {
    calls: Vec<String>,
}

impl CapHost for CountingHost {
    fn call(&mut self, cap: &str) -> Result<i64, String> {
        self.calls.push(cap.to_string());
        Ok(i64::try_from(self.calls.len()).unwrap_or(i64::MAX))
    }
}

fn eval_l2(path: &std::path::Path) -> Result<(), agenterm_chassis::ChassisError> {
    let raw = std::fs::read_to_string(path)?;
    let source: L2Source = serde_json::from_str(&raw)?;
    let program = assemble(&source, None).map_err(agenterm_chassis::ChassisError::Check)?;
    let mut host = CountingHost { calls: Vec::new() };
    let value = run(&program, &mut host, DEFAULT_MAX_STEPS)
        .map_err(agenterm_chassis::ChassisError::Check)?;
    println!(
        "{}",
        serde_json::json!({
            "value": value,
            "caps_called": host.calls,
            "bytes": program.code.len(),
        })
    );
    Ok(())
}

fn take_opt(args: &[String], name: &str) -> Option<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| PathBuf::from(&pair[1]))
}
