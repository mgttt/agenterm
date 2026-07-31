use std::{
    cell::RefCell,
    fs::{DirEntry, FileType, Metadata},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rhai::{Array, Dynamic, Engine, EvalAltResult, Module, Shared};
use sha2::{Digest, Sha256};

thread_local! {
    static INVOCATION_TEMP_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

static ATOMIC_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const MAX_BYTES_VALUE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ScriptPath(pub(crate) PathBuf);

#[derive(Clone, Debug)]
pub struct ScriptBytes(pub(crate) Vec<u8>);

#[derive(Clone, Debug)]
pub struct ScriptDirEntry {
    path: PathBuf,
    file_type: FileType,
}

#[derive(Clone, Debug)]
pub struct ScriptMetadata(Metadata);

#[derive(Clone, Copy, Debug)]
pub struct ScriptSystemTime(SystemTime);

pub struct InvocationTempScope {
    previous: Option<PathBuf>,
}

pub fn enter_invocation_temp_root(root: Option<&Path>) -> InvocationTempScope {
    let previous =
        INVOCATION_TEMP_ROOT.with(|current| current.replace(root.map(Path::to_path_buf)));
    InvocationTempScope { previous }
}

impl Drop for InvocationTempScope {
    fn drop(&mut self) {
        INVOCATION_TEMP_ROOT.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

pub fn register_local(engine: &mut Engine) {
    crate::script_stream::register(engine);

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
    engine.register_fn("get", bytes_get);
    engine.register_fn("slice", bytes_slice);
    engine.register_fn("append", bytes_append);
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
    engine.register_get("is_symlink", |metadata: &mut ScriptMetadata| {
        metadata.0.file_type().is_symlink()
    });
    engine.register_get("is_reparse_point", metadata_is_reparse_point);
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
    path.set_native_fn("parent", path_parent);

    let mut fs = Module::new();
    fs.set_native_fn("read_to_string", fs_read_to_string);
    fs.set_native_fn("read", fs_read);
    fs.set_native_fn("write", fs_write);
    fs.set_native_fn("write_bytes", fs_write_bytes);
    fs.set_native_fn("exists", fs_exists);
    fs.set_native_fn("metadata", fs_metadata);
    fs.set_native_fn("symlink_metadata", fs_symlink_metadata);
    fs.set_native_fn("read_dir", fs_read_dir);
    fs.set_native_fn("create_dir", fs_create_dir);
    fs.set_native_fn("create_dir_all", fs_create_dir_all);
    fs.set_native_fn("copy", fs_copy);
    fs.set_native_fn("rename", fs_rename);
    fs.set_native_fn("remove_file", fs_remove_file);
    fs.set_native_fn("remove_dir", fs_remove_dir);
    fs.set_native_fn("remove_dir_all", fs_remove_dir_all);

    let mut system_time = Module::new();
    system_time.set_native_fn("now", system_time_now);
    let mut time = Module::new();
    time.set_sub_module("SystemTime", system_time);

    let mut std_module = Module::new();
    std_module.set_sub_module("fs", fs);
    std_module.set_sub_module("path", path);
    crate::script_process::register(engine, &mut std_module, &mut time);
    crate::script_net::register(engine, &mut std_module);
    std_module.set_sub_module("time", time);
    engine.register_static_module("std", Shared::new(std_module));

    let mut json = Module::new();
    json.set_native_fn("parse", json_parse);
    json.set_native_fn("parse_file", json_parse_file);
    json.set_native_fn("stringify", json_stringify);
    json.set_native_fn("stringify_pretty", json_stringify_pretty);

    let mut bytes = Module::new();
    bytes.set_native_fn("from_text", bytes_from_text);
    bytes.set_native_fn("from_array", bytes_from_array);

    let mut crypto = Module::new();
    crypto.set_native_fn("sha256", crypto_sha256);
    crypto.set_native_fn("sha256_file", crypto_sha256_file);

    let mut hash = Module::new();
    hash.set_native_fn("fnv1a64", hash_fnv1a64);

    let mut runtime = Module::new();
    runtime.set_native_fn("temp_dir", runtime_temp_dir);
    runtime.set_native_fn("atomic_write", runtime_atomic_write);
    runtime.set_native_fn("atomic_write_bytes", runtime_atomic_write_bytes);
    runtime.set_native_fn("append_sync", runtime_append_sync);
    runtime.set_native_fn("append_sync_bytes", runtime_append_sync_bytes);

    let mut rhai_module = Module::new();
    rhai_module.set_sub_module("json", json);
    rhai_module.set_sub_module("bytes", bytes);
    rhai_module.set_sub_module("crypto", crypto);
    rhai_module.set_sub_module("hash", hash);
    rhai_module.set_sub_module("runtime", runtime);
    crate::script_clipboard::register(&mut rhai_module);
    crate::script_image::register(engine, &mut rhai_module);
    crate::script_task::register(engine, &mut rhai_module);
    crate::script_http::register(engine, &mut rhai_module);
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

fn path_parent(value: &str) -> Result<ScriptPath, Box<EvalAltResult>> {
    PathBuf::from(value)
        .parent()
        .map(|parent| ScriptPath(parent.to_path_buf()))
        .ok_or_else(|| {
            Box::new(EvalAltResult::ErrorRuntime(
                format!("path_parent: path has no parent: {value}").into(),
                rhai::Position::NONE,
            ))
        })
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

fn crypto_sha256(value: ScriptBytes) -> Result<String, Box<EvalAltResult>> {
    Ok(sha256_hex(Sha256::digest(value.0)))
}

fn crypto_sha256_file(path: &str) -> Result<String, Box<EvalAltResult>> {
    let mut input =
        std::fs::File::open(path).map_err(|error| io_error("crypto_sha256_file", path, error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = input
            .read(&mut buffer)
            .map_err(|error| io_error("crypto_sha256_file", path, error))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(sha256_hex(digest.finalize()))
}

fn hash_fnv1a64(value: ScriptBytes) -> Result<String, Box<EvalAltResult>> {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in value.0 {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}

fn fs_exists(path: &str) -> Result<bool, Box<EvalAltResult>> {
    Ok(std::path::Path::new(path).exists())
}

fn fs_metadata(path: &str) -> Result<ScriptMetadata, Box<EvalAltResult>> {
    std::fs::metadata(path)
        .map(ScriptMetadata)
        .map_err(|error| io_error("fs_metadata", path, error))
}

fn fs_symlink_metadata(path: &str) -> Result<ScriptMetadata, Box<EvalAltResult>> {
    std::fs::symlink_metadata(path)
        .map(ScriptMetadata)
        .map_err(|error| io_error("fs_symlink_metadata", path, error))
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

fn fs_create_dir(path: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::create_dir(path).map_err(|error| io_error("fs_create_dir", path, error))
}

fn fs_create_dir_all(path: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::create_dir_all(path).map_err(|error| io_error("fs_create_dir_all", path, error))
}

fn fs_copy(source: &str, destination: &str) -> Result<rhai::INT, Box<EvalAltResult>> {
    let bytes = std::fs::copy(source, destination)
        .map_err(|error| io_error("fs_copy", destination, error))?;
    rhai::INT::try_from(bytes)
        .map_err(|_| "fs_copy_overflow: byte count exceeds Rhai integer".into())
}

fn fs_rename(source: &str, destination: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::rename(source, destination).map_err(|error| io_error("fs_rename", destination, error))
}

fn fs_remove_file(path: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::remove_file(path).map_err(|error| io_error("fs_remove_file", path, error))
}

fn fs_remove_dir(path: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::remove_dir(path).map_err(|error| io_error("fs_remove_dir", path, error))
}

fn fs_remove_dir_all(path: &str) -> Result<(), Box<EvalAltResult>> {
    std::fs::remove_dir_all(path).map_err(|error| io_error("fs_remove_dir_all", path, error))
}

fn runtime_temp_dir() -> Result<ScriptPath, Box<EvalAltResult>> {
    INVOCATION_TEMP_ROOT.with(|root| {
        root.borrow().clone().map(ScriptPath).ok_or_else(|| {
            "runtime_temp_unavailable: invocation has no owned temporary root".into()
        })
    })
}

fn runtime_atomic_write(path: &str, value: &str) -> Result<(), Box<EvalAltResult>> {
    atomic_write(path, value.as_bytes())
}

fn runtime_atomic_write_bytes(path: &str, value: ScriptBytes) -> Result<(), Box<EvalAltResult>> {
    atomic_write(path, &value.0)
}

fn runtime_append_sync(path: &str, value: &str) -> Result<(), Box<EvalAltResult>> {
    append_sync(path, value.as_bytes())
}

fn runtime_append_sync_bytes(path: &str, value: ScriptBytes) -> Result<(), Box<EvalAltResult>> {
    append_sync(path, &value.0)
}

fn append_sync(path: &str, value: &[u8]) -> Result<(), Box<EvalAltResult>> {
    if value.len() > MAX_BYTES_VALUE_BYTES {
        return Err(
            format!("runtime_append_too_large: maximum is {MAX_BYTES_VALUE_BYTES} bytes").into(),
        );
    }

    let destination = std::path::absolute(Path::new(path))
        .map_err(|error| io_error("runtime_append_open", path, error))?;
    let parent = destination
        .parent()
        .ok_or("runtime_append_invalid_target: parent directory required")?;
    let (mut output, created) = match std::fs::OpenOptions::new().append(true).open(&destination) {
        Ok(output) => (output, false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&destination)
                .map_err(|error| io_error("runtime_append_open", path, error))?,
            true,
        ),
        Err(error) => return Err(io_error("runtime_append_open", path, error)),
    };
    output
        .write_all(value)
        .map_err(|error| io_error("runtime_append_write", path, error))?;
    output
        .sync_all()
        .map_err(|error| io_error("runtime_append_sync", path, error))?;
    if created {
        sync_parent(parent).map_err(|error| io_error("runtime_append_parent_sync", path, error))?;
    }
    Ok(())
}

fn atomic_write(path: &str, value: &[u8]) -> Result<(), Box<EvalAltResult>> {
    let destination = std::path::absolute(Path::new(path))
        .map_err(|error| io_error("runtime_atomic_write", path, error))?;
    let parent = destination
        .parent()
        .ok_or("runtime_atomic_write_invalid_target: parent directory required")?;
    let name = destination
        .file_name()
        .ok_or("runtime_atomic_write_invalid_target: file name required")?
        .to_string_lossy();
    let sequence = ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".{name}.agenterm-atomic-{}-{sequence}",
        std::process::id()
    ));
    let mut cleanup = AtomicWriteCleanup {
        path: temporary.clone(),
        armed: true,
    };
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| io_error("runtime_atomic_write_create", path, error))?;
    output
        .write_all(value)
        .and_then(|_| output.sync_all())
        .map_err(|error| io_error("runtime_atomic_write_data", path, error))?;
    drop(output);
    replace_file(&temporary, &destination)
        .map_err(|error| io_error("runtime_atomic_write_promote", path, error))?;
    cleanup.armed = false;
    sync_parent(parent).map_err(|error| io_error("runtime_atomic_write_sync", path, error))?;
    Ok(())
}

struct AtomicWriteCleanup {
    path: PathBuf,
    armed: bool,
}

impl Drop for AtomicWriteCleanup {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    // Rust's filesystem APIs opt into verbatim paths internally, but this
    // direct Win32 call must do so explicitly. Canonicalizing the existing
    // source and destination parent produces `\\?\` paths and keeps atomic
    // promotion working beyond the legacy MAX_PATH boundary.
    let source = std::fs::canonicalize(source)?;
    let destination = std::fs::canonicalize(
        destination
            .parent()
            .ok_or_else(|| std::io::Error::other("destination parent required"))?,
    )?
    .join(
        destination
            .file_name()
            .ok_or_else(|| std::io::Error::other("destination name required"))?,
    );
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    Ok(())
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

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &mut ScriptMetadata) -> Result<bool, Box<EvalAltResult>> {
    use std::os::windows::fs::MetadataExt;

    Ok(windows_attributes_are_reparse(metadata.0.file_attributes()))
}

#[cfg(windows)]
fn windows_attributes_are_reparse(attributes: u32) -> bool {
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(metadata: &mut ScriptMetadata) -> Result<bool, Box<EvalAltResult>> {
    Ok(metadata.0.file_type().is_symlink())
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

fn bytes_from_array(values: Array) -> Result<ScriptBytes, Box<EvalAltResult>> {
    if values.len() > MAX_BYTES_VALUE_BYTES {
        return Err(format!("bytes_length_limit: maximum is {MAX_BYTES_VALUE_BYTES} bytes").into());
    }
    let mut bytes = Vec::with_capacity(values.len());
    for value in values {
        let value = value
            .try_cast::<rhai::INT>()
            .ok_or("bytes_value_type: expected integer")?;
        bytes.push(u8::try_from(value).map_err(|_| "bytes_value_range: expected 0..255")?);
    }
    Ok(ScriptBytes(bytes))
}

fn bytes_get(value: &mut ScriptBytes, index: rhai::INT) -> Result<rhai::INT, Box<EvalAltResult>> {
    let index = usize::try_from(index)
        .ok()
        .filter(|index| *index < value.0.len())
        .ok_or("bytes_index_out_of_bounds")?;
    Ok(rhai::INT::from(value.0[index]))
}

fn bytes_slice(
    value: &mut ScriptBytes,
    offset: rhai::INT,
    length: rhai::INT,
) -> Result<ScriptBytes, Box<EvalAltResult>> {
    let offset = usize::try_from(offset).map_err(|_| "bytes_slice_out_of_bounds")?;
    let length = usize::try_from(length).map_err(|_| "bytes_slice_out_of_bounds")?;
    let end = offset
        .checked_add(length)
        .filter(|end| *end <= value.0.len())
        .ok_or("bytes_slice_out_of_bounds")?;
    Ok(ScriptBytes(value.0[offset..end].to_vec()))
}

fn bytes_append(value: &mut ScriptBytes, other: ScriptBytes) -> Result<(), Box<EvalAltResult>> {
    value
        .0
        .len()
        .checked_add(other.0.len())
        .filter(|length| *length <= MAX_BYTES_VALUE_BYTES)
        .ok_or("bytes_length_limit")?;
    value.0.extend(other.0);
    Ok(())
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

fn json_parse_file(path: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    const MAX_JSON_FILE_BYTES: u64 = 8 * 1024 * 1024;
    let file =
        std::fs::File::open(path).map_err(|error| io_error("json_parse_file", path, error))?;
    let length = file
        .metadata()
        .map_err(|error| io_error("json_parse_file", path, error))?
        .len();
    if length > MAX_JSON_FILE_BYTES {
        return Err(
            format!("json_parse_file_too_large: maximum is {MAX_JSON_FILE_BYTES} bytes").into(),
        );
    }
    let value: serde_json::Value =
        serde_json::from_reader(file).map_err(|error| format!("json_parse_file: {error}"))?;
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
        assert_eq!(
            engine
                .eval::<String>(
                    r#"
                        let bytes = rhai::bytes::from_text("hello");
                        let tail = bytes.slice(1, 2);
                        bytes.append(rhai::bytes::from_text("!"));
                        `${bytes.get(0)}:${tail.to_text()}:${bytes.to_text()}`
                    "#,
                )
                .unwrap(),
            "104:el:hello!"
        );
        assert_eq!(
            engine
                .eval::<String>(
                    r#"
                        let bytes = rhai::bytes::from_array([0, 127, 128, 255]);
                        `${bytes.len}:${bytes.get(0)}:${bytes.get(3)}`
                    "#,
                )
                .unwrap(),
            "4:0:255"
        );
        assert!(
            engine
                .eval::<Dynamic>("rhai::bytes::from_array([256])")
                .unwrap_err()
                .to_string()
                .contains("bytes_value_range")
        );
    }

    #[test]
    fn json_parse_file_streams_a_large_explicit_document() {
        let path = std::env::temp_dir().join(format!(
            "agenterm-json-parse-file-{}.json",
            std::process::id()
        ));
        let payload = "x".repeat(300_000);
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({ "payload": payload })).unwrap(),
        )
        .unwrap();
        let script_path = path.to_string_lossy().replace('\\', "\\\\");
        let value = local_engine()
            .eval::<rhai::Map>(&format!(r#"rhai::json::parse_file("{script_path}")"#))
            .unwrap();
        assert_eq!(
            value["payload"].clone().into_string().unwrap().len(),
            300_000
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn crypto_sha256_hashes_typed_bytes_and_streamed_files_identically() {
        let expected = concat!(
            "ba7816bf8f01cfea414140de5dae2223",
            "b00361a396177a9cb410ff61f20015ad"
        );
        let engine = local_engine();
        assert_eq!(
            engine
                .eval::<String>(r#"rhai::crypto::sha256(rhai::bytes::from_text("abc"))"#)
                .unwrap(),
            expected
        );

        let fixture = std::env::temp_dir().join(format!(
            "agenterm-script-sha256-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&fixture, b"abc").unwrap();
        let path = fixture.to_string_lossy().replace('\\', "\\\\");
        assert_eq!(
            engine
                .eval::<String>(&format!(r#"rhai::crypto::sha256_file("{path}")"#))
                .unwrap(),
            expected
        );
        std::fs::remove_file(fixture).unwrap();
    }

    #[test]
    fn fnv1a64_hash_is_fixed_width_and_wire_compatible() {
        assert_eq!(
            local_engine()
                .eval::<String>(r#"rhai::hash::fnv1a64(rhai::bytes::from_text("abc"))"#)
                .unwrap(),
            "fnv1a64:e71fa2190541574b"
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
                let link_metadata = std::fs::symlink_metadata(entry.path.display);
                #{{
                    count: entries.len,
                    file_name: entry.file_name,
                    is_file: entry.is_file,
                    bytes: metadata.len,
                    is_symlink: link_metadata.is_symlink,
                    is_reparse_point: link_metadata.is_reparse_point,
                    absolute: std::path::absolute(entry.path.display).is_absolute,
                    parent: std::path::parent(entry.path.display).display,
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
        assert!(!result["is_symlink"].as_bool().unwrap());
        assert!(!result["is_reparse_point"].as_bool().unwrap());
        assert!(result["absolute"].as_bool().unwrap());
        assert_eq!(
            result["parent"].clone().into_string().unwrap(),
            directory.to_string_lossy()
        );
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

    #[cfg(windows)]
    #[test]
    fn windows_reparse_attribute_detection_is_exact() {
        assert!(!windows_attributes_are_reparse(0));
        assert!(windows_attributes_are_reparse(0x0000_0400));
        assert!(windows_attributes_are_reparse(0x0000_0400 | 0x10));
    }

    #[test]
    fn filesystem_lifecycle_is_typed_and_unrestricted() {
        let root =
            std::env::temp_dir().join(format!("agenterm-script-lifecycle-{}", std::process::id()));
        let nested = root.join("nested");
        let source = nested.join("source.txt");
        let copied = nested.join("copied.txt");
        let renamed = nested.join("renamed.txt");
        let literal = |path: &Path| path.to_string_lossy().replace('\\', "\\\\");
        let script = format!(
            r#"
                std::fs::create_dir_all("{}");
                std::fs::write("{}", "hello");
                let copied = std::fs::copy("{}", "{}");
                std::fs::rename("{}", "{}");
                std::fs::remove_file("{}");
                std::fs::remove_dir_all("{}");
                copied
            "#,
            literal(&nested),
            literal(&source),
            literal(&source),
            literal(&copied),
            literal(&copied),
            literal(&renamed),
            literal(&source),
            literal(&root),
        );
        assert_eq!(local_engine().eval::<rhai::INT>(&script).unwrap(), 5);
        assert!(!root.exists());
    }

    #[test]
    fn filesystem_root_reaches_the_os_without_a_runtime_policy_filter() {
        let current = std::env::current_dir().unwrap();
        let root = current.ancestors().last().unwrap();
        let error = fs_remove_file(&root.to_string_lossy())
            .unwrap_err()
            .to_string();
        assert!(error.contains("fs_remove_file:"));
        assert!(!error.contains("broad_target"));
        assert!(root.exists());
    }

    #[test]
    fn invocation_temp_and_atomic_replace_have_explicit_lifecycle() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-script-runtime-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("result-目录.json");
        std::fs::write(&destination, "old").unwrap();
        let literal = |path: &Path| path.to_string_lossy().replace('\\', "\\\\");
        {
            let _scope = enter_invocation_temp_root(Some(&root));
            let source = format!(
                r#"
                    let temp = rhai::runtime::temp_dir();
                    rhai::runtime::atomic_write("{}", `{{"value":"新"}}`);
                    #{{ temp: temp.display, value: std::fs::read_to_string("{}") }}
                "#,
                literal(&destination),
                literal(&destination),
            );
            let result = local_engine()
                .eval::<rhai::Map>(&source)
                .expect("owned temp and atomic replacement");
            assert_eq!(
                result["temp"].clone().into_string().unwrap(),
                root.display().to_string()
            );
            assert_eq!(
                result["value"].clone().into_string().unwrap(),
                r#"{"value":"新"}"#
            );
        }
        let error = local_engine()
            .eval::<ScriptPath>("rhai::runtime::temp_dir()")
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime_temp_unavailable"));
        assert_eq!(
            std::fs::read_to_string(&destination).unwrap(),
            r#"{"value":"新"}"#
        );
        assert!(
            std::fs::read_dir(&root).unwrap().all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("atomic")),
            "atomic promotion must not leave a sibling staging file"
        );
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn runtime_append_sync_preserves_existing_content_and_record_order() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-script-append-order-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("records.bin");
        std::fs::write(&destination, b"existing:").unwrap();
        let path = destination.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"
                rhai::runtime::append_sync("{path}", "text-界");
                rhai::runtime::append_sync_bytes(
                    "{path}",
                    rhai::bytes::from_array([0, 255, 10])
                );
                rhai::runtime::append_sync("{path}", "tail");
            "#
        );
        local_engine().run(&source).unwrap();

        let mut expected = "existing:text-界".as_bytes().to_vec();
        expected.extend_from_slice(&[0, 255, 10]);
        expected.extend_from_slice(b"tail");
        assert_eq!(std::fs::read(&destination).unwrap(), expected);
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn runtime_append_sync_durably_creates_a_missing_file() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-script-append-create-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let destination = root.join("created.log");
        let path = destination.to_string_lossy().replace('\\', "\\\\");
        {
            let engine = local_engine();
            engine
                .run(&format!(
                    r#"rhai::runtime::append_sync("{path}", "first");"#
                ))
                .unwrap();
        }

        assert_eq!(std::fs::read(&destination).unwrap(), b"first");
        runtime_append_sync_bytes(&destination.to_string_lossy(), ScriptBytes(vec![0, 1, 2]))
            .unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"first\0\x01\x02");
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn runtime_append_sync_bounds_records_and_reports_path_failures() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-script-append-errors-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let destination = root.join("missing-parent").join("record.log");
        let path = destination.to_string_lossy().replace('\\', "\\\\");
        let error = local_engine()
            .run(&format!(r#"rhai::runtime::append_sync("{path}", "x");"#))
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime_append_open"));
        assert!(error.contains("record.log"));

        let oversized = vec![0; MAX_BYTES_VALUE_BYTES + 1];
        let error = runtime_append_sync_bytes(&path, ScriptBytes(oversized))
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime_append_too_large"));
        assert!(!root.exists(), "a rejected record must not create its path");

        let oversized_text = "界".repeat(MAX_BYTES_VALUE_BYTES / 3 + 1);
        let error = runtime_append_sync(&path, &oversized_text)
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime_append_too_large"));
        assert!(!root.exists(), "a rejected record must not create its path");

        std::fs::create_dir(&root).unwrap();
        let error = runtime_append_sync(&root.to_string_lossy(), "x")
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime_append_open"));
        std::fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn atomic_replace_supports_verbatim_paths_beyond_max_path() {
        let root = std::env::temp_dir().join(format!(
            "agenterm-script-long-atomic-{}-{}",
            std::process::id(),
            ATOMIC_WRITE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut deep = root.clone();
        while deep.as_os_str().len() < 280 {
            deep.push("qualified-package-boundary-segment");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let destination = deep.join("package-manifest.json");
        atomic_write(&destination.to_string_lossy(), b"first").unwrap();
        atomic_write(&destination.to_string_lossy(), b"second").unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"second");
        std::fs::remove_dir_all(root).unwrap();
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
