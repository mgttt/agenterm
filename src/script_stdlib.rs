use std::{
    fs::{DirEntry, FileType, Metadata},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use rhai::{Array, Dynamic, Engine, EvalAltResult, Module, Shared};

#[derive(Clone, Debug)]
pub struct ScriptPath(PathBuf);

#[derive(Clone, Debug)]
pub struct ScriptBytes(Vec<u8>);

#[derive(Clone, Debug)]
pub struct ScriptDirEntry {
    path: PathBuf,
    file_type: FileType,
}

#[derive(Clone, Debug)]
pub struct ScriptMetadata(Metadata);

#[derive(Clone, Copy, Debug)]
pub struct ScriptSystemTime(SystemTime);

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

    engine.register_type_with_name::<ScriptDirEntry>("DirEntry");
    engine.register_get("path", |entry: &mut ScriptDirEntry| {
        ScriptPath(entry.path.clone())
    });
    engine.register_get("file_name", |entry: &mut ScriptDirEntry| {
        entry
            .path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    engine.register_get("is_file", |entry: &mut ScriptDirEntry| {
        entry.file_type.is_file()
    });
    engine.register_get("is_dir", |entry: &mut ScriptDirEntry| {
        entry.file_type.is_dir()
    });
    engine.register_get("is_symlink", |entry: &mut ScriptDirEntry| {
        entry.file_type.is_symlink()
    });
    engine.register_get("metadata", dir_entry_metadata);

    engine.register_type_with_name::<ScriptMetadata>("Metadata");
    engine.register_get("is_file", |metadata: &mut ScriptMetadata| {
        metadata.0.is_file()
    });
    engine.register_get("is_dir", |metadata: &mut ScriptMetadata| {
        metadata.0.is_dir()
    });
    engine.register_get("len", metadata_len);
    engine.register_get("modified", metadata_modified);

    engine.register_type_with_name::<ScriptSystemTime>("SystemTime");
    engine.register_get("unix_millis", system_time_unix_millis);
    engine.register_get("rfc3339", system_time_rfc3339);

    let mut path_buf = Module::new();
    path_buf.set_native_fn("from", path_from);
    let mut path = Module::new();
    path.set_sub_module("PathBuf", path_buf);
    path.set_native_fn("join", path_join);
    path.set_native_fn("absolute", path_absolute);

    let mut fs = Module::new();
    fs.set_native_fn("read_to_string", fs_read_to_string);
    fs.set_native_fn("read", fs_read);
    fs.set_native_fn("write", fs_write);
    fs.set_native_fn("write_bytes", fs_write_bytes);
    fs.set_native_fn("exists", fs_exists);
    fs.set_native_fn("metadata", fs_metadata);
    fs.set_native_fn("read_dir", fs_read_dir);

    let mut system_time = Module::new();
    system_time.set_native_fn("now", system_time_now);
    let mut time = Module::new();
    time.set_sub_module("SystemTime", system_time);

    let mut std_module = Module::new();
    std_module.set_sub_module("fs", fs);
    std_module.set_sub_module("path", path);
    std_module.set_sub_module("time", time);
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

fn path_absolute(value: &str) -> Result<ScriptPath, Box<EvalAltResult>> {
    std::path::absolute(value)
        .map(ScriptPath)
        .map_err(|error| io_error("path_absolute", value, error))
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

fn fs_metadata(path: &str) -> Result<ScriptMetadata, Box<EvalAltResult>> {
    std::fs::metadata(path)
        .map(ScriptMetadata)
        .map_err(|error| io_error("fs_metadata", path, error))
}

fn fs_read_dir(path: &str) -> Result<Array, Box<EvalAltResult>> {
    let entries = std::fs::read_dir(path).map_err(|error| io_error("fs_read_dir", path, error))?;
    entries
        .map(|entry| {
            let entry = entry.map_err(|error| io_error("fs_read_dir_entry", path, error))?;
            script_dir_entry(entry).map(Dynamic::from)
        })
        .collect()
}

fn script_dir_entry(entry: DirEntry) -> Result<ScriptDirEntry, Box<EvalAltResult>> {
    let path = entry.path();
    let file_type = entry
        .file_type()
        .map_err(|error| io_error("fs_dir_entry_type", &path.to_string_lossy(), error))?;
    Ok(ScriptDirEntry { path, file_type })
}

fn dir_entry_metadata(entry: &mut ScriptDirEntry) -> Result<ScriptMetadata, Box<EvalAltResult>> {
    std::fs::metadata(&entry.path)
        .map(ScriptMetadata)
        .map_err(|error| {
            io_error(
                "fs_dir_entry_metadata",
                &entry.path.to_string_lossy(),
                error,
            )
        })
}

fn metadata_len(metadata: &mut ScriptMetadata) -> Result<rhai::INT, Box<EvalAltResult>> {
    rhai::INT::try_from(metadata.0.len())
        .map_err(|_| "filesystem_metadata_overflow: file length exceeds Rhai integer".into())
}

fn metadata_modified(
    metadata: &mut ScriptMetadata,
) -> Result<ScriptSystemTime, Box<EvalAltResult>> {
    metadata
        .0
        .modified()
        .map(ScriptSystemTime)
        .map_err(|error| format!("filesystem_modified_unavailable: {error}").into())
}

fn system_time_now() -> Result<ScriptSystemTime, Box<EvalAltResult>> {
    Ok(ScriptSystemTime(SystemTime::now()))
}

fn system_time_unix_millis(value: &mut ScriptSystemTime) -> Result<rhai::INT, Box<EvalAltResult>> {
    let millis = value
        .0
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system_time_before_unix_epoch: {error}"))?
        .as_millis();
    rhai::INT::try_from(millis)
        .map_err(|_| "system_time_overflow: milliseconds exceed Rhai integer".into())
}

fn system_time_rfc3339(value: &mut ScriptSystemTime) -> Result<String, Box<EvalAltResult>> {
    let duration = value
        .0
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system_time_before_unix_epoch: {error}"))?;
    let seconds = i64::try_from(duration.as_secs())
        .map_err(|_| "system_time_overflow: seconds exceed supported range")?;
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_date_from_unix_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        duration.subsec_millis()
    ))
}

fn civil_date_from_unix_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
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

    #[test]
    fn directory_metadata_absolute_paths_and_system_time_are_typed() {
        let directory =
            std::env::temp_dir().join(format!("agenterm-script-directory-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("entry.txt");
        std::fs::write(&file, "hello").unwrap();
        let script_directory = directory.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"
                let entries = std::fs::read_dir("{script_directory}");
                let entry = entries[0];
                let metadata = std::fs::metadata(entry.path.display);
                #{{
                    count: entries.len,
                    file_name: entry.file_name,
                    is_file: entry.is_file,
                    bytes: metadata.len,
                    absolute: std::path::absolute(entry.path.display).is_absolute,
                    modified: metadata.modified.unix_millis,
                    modified_text: metadata.modified.rfc3339,
                    now: std::time::SystemTime::now().unix_millis
                }}
            "#
        );
        let result = local_engine()
            .eval::<rhai::Map>(&source)
            .expect("typed directory facts");
        assert_eq!(result["count"].as_int().unwrap(), 1);
        assert_eq!(
            result["file_name"].clone().into_string().unwrap(),
            "entry.txt"
        );
        assert!(result["is_file"].as_bool().unwrap());
        assert_eq!(result["bytes"].as_int().unwrap(), 5);
        assert!(result["absolute"].as_bool().unwrap());
        assert!(result["modified"].as_int().unwrap() > 0);
        assert!(
            result["modified_text"]
                .clone()
                .into_string()
                .unwrap()
                .ends_with('Z')
        );
        assert!(
            result["now"].as_int().unwrap() >= result["modified"].as_int().unwrap(),
            "current wall clock must not precede a newly written fixture"
        );
        std::fs::remove_file(&file).unwrap();
        std::fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn system_time_rfc3339_uses_utc_and_millisecond_precision() {
        let mut epoch = ScriptSystemTime(UNIX_EPOCH);
        assert_eq!(
            system_time_rfc3339(&mut epoch).unwrap(),
            "1970-01-01T00:00:00.000Z"
        );
        let mut leap_day =
            ScriptSystemTime(UNIX_EPOCH + std::time::Duration::from_secs(951_782_400));
        assert_eq!(
            system_time_rfc3339(&mut leap_day).unwrap(),
            "2000-02-29T00:00:00.000Z"
        );
    }
}
