use std::path::PathBuf;

use rhai::{Dynamic, Engine, EvalAltResult, Module, Shared};

#[derive(Clone, Debug)]
pub struct ScriptPath(PathBuf);

#[derive(Clone, Debug)]
pub struct ScriptBytes(Vec<u8>);

pub fn register_local(engine: &mut Engine) {
    engine.register_type_with_name::<ScriptPath>("PathBuf");
    engine.register_get("display", |path: &mut ScriptPath| {
        path.0.to_string_lossy().into_owned()
    });
    engine.register_get("file_name", |path: &mut ScriptPath| {
        path.0
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    engine.register_get("extension", |path: &mut ScriptPath| {
        path.0
            .extension()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    engine.register_get("is_absolute", |path: &mut ScriptPath| path.0.is_absolute());
    engine.register_fn("join", |path: &mut ScriptPath, child: &str| {
        path.0.push(child);
    });

    engine.register_type_with_name::<ScriptBytes>("Bytes");
    engine.register_get("len", |bytes: &mut ScriptBytes| bytes.0.len() as rhai::INT);
    engine.register_fn("to_text", bytes_to_text);

    let mut path_buf = Module::new();
    path_buf.set_native_fn("from", path_from);
    let mut path = Module::new();
    path.set_sub_module("PathBuf", path_buf);
    path.set_native_fn("join", path_join);

    let mut fs = Module::new();
    fs.set_native_fn("read_to_string", fs_read_to_string);
    fs.set_native_fn("read", fs_read);
    fs.set_native_fn("write", fs_write);
    fs.set_native_fn("write_bytes", fs_write_bytes);
    fs.set_native_fn("exists", fs_exists);

    let mut std_module = Module::new();
    std_module.set_sub_module("fs", fs);
    std_module.set_sub_module("path", path);
    engine.register_static_module("std", Shared::new(std_module));

    let mut json = Module::new();
    json.set_native_fn("parse", json_parse);
    json.set_native_fn("stringify", json_stringify);
    json.set_native_fn("stringify_pretty", json_stringify_pretty);

    let mut bytes = Module::new();
    bytes.set_native_fn("from_text", bytes_from_text);

    let mut rhai_module = Module::new();
    rhai_module.set_sub_module("json", json);
    rhai_module.set_sub_module("bytes", bytes);
    engine.register_static_module("rhai", Shared::new(rhai_module));
}

fn path_from(value: &str) -> Result<ScriptPath, Box<EvalAltResult>> {
    Ok(ScriptPath(PathBuf::from(value)))
}

fn path_join(parent: &str, child: &str) -> Result<ScriptPath, Box<EvalAltResult>> {
    Ok(ScriptPath(PathBuf::from(parent).join(child)))
}

fn fs_read_to_string(path: &str) -> Result<String, Box<EvalAltResult>> {
    std::fs::read_to_string(path).map_err(|error| io_error("fs_read_to_string", path, error))
}

fn fs_read(path: &str) -> Result<ScriptBytes, Box<EvalAltResult>> {
    std::fs::read(path)
        .map(ScriptBytes)
        .map_err(|error| io_error("fs_read", path, error))
}

fn fs_write(path: &str, value: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::write(path, value).map_err(|error| io_error("fs_write", path, error))
}

fn fs_write_bytes(path: &str, value: ScriptBytes) -> Result<(), Box<EvalAltResult>> {
    std::fs::write(path, value.0).map_err(|error| io_error("fs_write", path, error))
}

fn fs_exists(path: &str) -> Result<bool, Box<EvalAltResult>> {
    Ok(std::path::Path::new(path).exists())
}

fn bytes_from_text(value: &str) -> Result<ScriptBytes, Box<EvalAltResult>> {
    Ok(ScriptBytes(value.as_bytes().to_vec()))
}

fn bytes_to_text(value: &mut ScriptBytes) -> Result<String, Box<EvalAltResult>> {
    String::from_utf8(value.0.clone())
        .map_err(|error| format!("bytes_invalid_utf8: {error}").into())
}

fn json_parse(value: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    let value: serde_json::Value =
        serde_json::from_str(value).map_err(|error| format!("json_parse: {error}"))?;
    rhai::serde::to_dynamic(value).map_err(|error| format!("json_dynamic: {error}").into())
}

fn json_stringify(value: Dynamic) -> Result<String, Box<EvalAltResult>> {
    let value: serde_json::Value =
        rhai::serde::from_dynamic(&value).map_err(|error| format!("json_value: {error}"))?;
    serde_json::to_string(&value).map_err(|error| format!("json_stringify: {error}").into())
}

fn json_stringify_pretty(value: Dynamic) -> Result<String, Box<EvalAltResult>> {
    let value: serde_json::Value =
        rhai::serde::from_dynamic(&value).map_err(|error| format!("json_value: {error}"))?;
    serde_json::to_string_pretty(&value).map_err(|error| format!("json_stringify: {error}").into())
}

fn io_error(code: &'static str, path: &str, error: std::io::Error) -> Box<EvalAltResult> {
    format!("{code}: {}: {error}", display_path(path)).into()
}

fn display_path(path: &str) -> String {
    let path = PathBuf::from(path);
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<path>".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_engine() -> Engine {
        let mut engine = Engine::new();
        register_local(&mut engine);
        engine
    }

    #[test]
    fn nested_path_json_and_bytes_modules_are_callable() {
        let engine = local_engine();
        assert_eq!(
            engine
                .eval::<String>(r#"std::path::PathBuf::from("folder/file.json").extension"#,)
                .unwrap(),
            "json"
        );
        assert_eq!(
            engine
                .eval::<String>(r#"rhai::json::stringify(rhai::json::parse(`{"answer":42}`))"#,)
                .unwrap(),
            r#"{"answer":42}"#
        );
        assert_eq!(
            engine
                .eval::<String>(r#"rhai::bytes::from_text("hello").to_text()"#)
                .unwrap(),
            "hello"
        );
    }

    #[test]
    fn local_filesystem_round_trip_and_error_are_bounded() {
        let directory =
            std::env::temp_dir().join(format!("agenterm-script-stdlib-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("round-trip.txt");
        let script_path = path.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"std::fs::write("{script_path}", "hello"); std::fs::read_to_string("{script_path}")"#
        );
        assert_eq!(local_engine().eval::<String>(&source).unwrap(), "hello");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&directory).unwrap();

        let error = local_engine()
            .eval::<String>(r#"std::fs::read_to_string("missing-secret-directory/file.txt")"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("fs_read_to_string"));
        assert!(error.contains("file.txt"));
        assert!(!error.contains("missing-secret-directory"));
    }
}
