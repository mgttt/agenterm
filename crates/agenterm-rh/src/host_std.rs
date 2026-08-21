//! `StdHost` — the Language-1 default host, **`std` only** (design D6 / D19).
//!
//! No `agenterm-platform`, no Fleet, no GUI/clipboard/task, no network, no
//! rustc. Everything here is `std` plus `sha2` (already a dependency) and
//! `serde_json`.
//!
//! This is Node-like and deliberately unrestricted for local use: a script run
//! with `StdHost` can read, write, and spawn. Sandboxing is done by supplying a
//! different `Host`, not by trimming this one (Security table).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::host::{Host, ProcessRequest};
use crate::lang_error::Error;
use crate::value::{HostObject, Value};

// ---------------------------------------------------------------- type ids

const TY_PATH: &str = "std.path.PathBuf";
const TY_METADATA: &str = "std.fs.Metadata";
const TY_DIRENTRY: &str = "std.fs.DirEntry";
const TY_SYSTEMTIME: &str = "std.time.SystemTime";
const TY_DURATION: &str = "std.time.Duration";
const TY_COMMAND: &str = "std.process.Command";
const TY_OUTPUT: &str = "std.process.Output";
const TY_CHILD: &str = "std.process.Child";
const TY_FILELOCK: &str = "std.fs.FileLock";

// ------------------------------------------------------------ object table

/// A command being built. `Command.arg(..)` mutates this in place; the script
/// keeps the same handle, which is why host objects need no write-back.
#[derive(Clone, Debug, Default)]
struct CommandSpec {
    program: String,
    args: Vec<String>,
    current_dir: Option<String>,
    env: Vec<(String, String)>,
    env_remove: Vec<String>,
    env_clear: bool,
    stdin_bytes: Option<Vec<u8>>,
    stdout_file: Option<String>,
    stderr_file: Option<String>,
    timeout_ms: u64,
    capture_limit: usize,
}

#[derive(Clone, Debug, Default)]
struct OutputData {
    status: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    truncated: bool,
    error: Option<String>,
}

enum Obj {
    Path(PathBuf),
    Metadata {
        meta: fs::Metadata,
    },
    DirEntry {
        path: PathBuf,
        name: String,
    },
    SystemTime(SystemTime),
    Duration(Duration),
    Command(CommandSpec),
    Output(OutputData),
    Child(Option<Child>),
    /// The binding owns the locked file for its scope; there are no methods.
    FileLock(#[allow(dead_code)] fs::File),
}

/// The Language-1 default host.
pub struct StdHost {
    argv: Vec<String>,
    objects: Vec<Obj>,
    fs_read_cap: usize,
    /// When set, `print` appends here instead of writing to stdout.
    captured: Option<String>,
}

impl Default for StdHost {
    fn default() -> Self {
        Self::new()
    }
}

impl StdHost {
    pub fn new() -> Self {
        Self {
            argv: Vec::new(),
            objects: Vec::new(),
            fs_read_cap: crate::host_api::RH_HOST_FS_READ_CAP as usize,
            captured: None,
        }
    }

    /// Script arguments, i.e. what `args.len` / `args[i]` see. This is **not**
    /// process argv: the CLI strips the interpreter and script path first.
    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.argv = args;
        self
    }

    #[must_use]
    pub fn with_fs_read_cap(mut self, cap: usize) -> Self {
        self.fs_read_cap = cap;
        self
    }

    /// Capture `print` output instead of writing to stdout.
    #[must_use]
    pub fn capturing(mut self) -> Self {
        self.captured = Some(String::new());
        self
    }

    /// The captured `print` output, if capturing.
    pub fn captured(&self) -> Option<&str> {
        self.captured.as_deref()
    }

    fn store(&mut self, object: Obj, type_id: &'static str) -> Value {
        self.objects.push(object);
        let handle = (self.objects.len() - 1) as u64;
        Value::Host(HostObject::new(type_id, handle))
    }

    fn get(&self, value: &Value) -> Result<&Obj, Error> {
        match value {
            Value::Host(object) => self
                .objects
                .get(object.handle() as usize)
                .ok_or_else(|| Error::Host("stale host object handle".to_owned())),
            other => Err(Error::Host(format!(
                "expected a host object, got {}",
                other.type_name()
            ))),
        }
    }

    fn get_mut(&mut self, value: &Value) -> Result<&mut Obj, Error> {
        match value {
            Value::Host(object) => {
                let handle = object.handle() as usize;
                self.objects
                    .get_mut(handle)
                    .ok_or_else(|| Error::Host("stale host object handle".to_owned()))
            }
            other => Err(Error::Host(format!(
                "expected a host object, got {}",
                other.type_name()
            ))),
        }
    }
}

// ------------------------------------------------------------- arg helpers

fn arg<'a>(args: &'a [Value], index: usize, name: &str) -> Result<&'a Value, Error> {
    args.get(index)
        .ok_or_else(|| Error::Host(format!("{name}: missing argument {index}")))
}

fn arg_str(args: &[Value], index: usize, name: &str) -> Result<String, Error> {
    match arg(args, index, name)? {
        Value::String(text) => Ok(text.clone()),
        other => Err(Error::Host(format!(
            "{name}: expected a string, got {}",
            other.type_name()
        ))),
    }
}

fn arg_int(args: &[Value], index: usize, name: &str) -> Result<i64, Error> {
    arg(args, index, name)?
        .as_int()
        .ok_or_else(|| Error::Host(format!("{name}: expected an int")))
}

fn arg_bytes(args: &[Value], index: usize, name: &str) -> Result<Vec<u8>, Error> {
    match arg(args, index, name)? {
        Value::Bytes(bytes) => Ok(bytes.clone()),
        Value::String(text) => Ok(text.as_bytes().to_vec()),
        other => Err(Error::Host(format!(
            "{name}: expected bytes, got {}",
            other.type_name()
        ))),
    }
}

fn io_err(name: &str, error: std::io::Error) -> Error {
    Error::Host(format!("{name}: {error}"))
}

// ---------------------------------------------------------------- rfc3339

/// Format a `SystemTime` as RFC 3339 UTC without pulling in a date crate.
fn rfc3339(time: SystemTime) -> String {
    let seconds = match time.duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_secs() as i64,
        Err(error) => -(error.duration().as_secs() as i64),
    };
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Howard Hinnant's `civil_from_days`, the standard shift-to-March algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn unix_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(delta) => i64::try_from(delta.as_millis()).unwrap_or(i64::MAX),
        Err(error) => -i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX),
    }
}

// ------------------------------------------------------------------- json

fn json_to_value(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Unit,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => Value::Int(n.as_i64().unwrap_or(0)),
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(items) => Value::Array(items.iter().map(json_to_value).collect()),
        serde_json::Value::Object(map) => Value::Map(
            map.iter()
                .map(|(key, value)| (key.clone(), json_to_value(value)))
                .collect(),
        ),
    }
}

fn value_to_json(value: &Value) -> Result<serde_json::Value, Error> {
    Ok(match value {
        Value::Unit => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Bytes(bytes) => serde_json::Value::Array(
            bytes
                .iter()
                .map(|b| serde_json::Value::Number((i64::from(*b)).into()))
                .collect(),
        ),
        Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(value_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Map(entries) => {
            let mut map = serde_json::Map::new();
            for (key, value) in entries {
                map.insert(key.clone(), value_to_json(value)?);
            }
            serde_json::Value::Object(map)
        }
        Value::Host(object) => {
            return Err(Error::Host(format!(
                "cannot serialise host object {}",
                object.type_id()
            )));
        }
    })
}

// ------------------------------------------------------------ Host impl

impl Host for StdHost {
    fn print(&mut self, text: &str) -> Result<(), Error> {
        match &mut self.captured {
            Some(buffer) => {
                buffer.push_str(text);
                buffer.push('\n');
            }
            None => println!("{text}"),
        }
        Ok(())
    }

    fn args_len(&self) -> Result<i64, Error> {
        Ok(i64::try_from(self.argv.len()).unwrap_or(i64::MAX))
    }

    fn arg(&self, index: u32) -> Result<String, Error> {
        Ok(self.argv.get(index as usize).cloned().unwrap_or_default())
    }

    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, Error> {
        match name {
            // ------------------------------------------------- std::env
            "std::env::current_dir" => Ok(Value::String(
                std::env::current_dir()
                    .map_err(|e| io_err(name, e))?
                    .display()
                    .to_string(),
            )),
            "std::env::get" => Ok(Value::String(
                std::env::var(arg_str(args, 0, name)?).unwrap_or_default(),
            )),
            // `has` returns an int, matching `env-has-get-probe.rh`'s `== 0`.
            "std::env::has" => Ok(Value::Int(i64::from(
                std::env::var_os(arg_str(args, 0, name)?).is_some(),
            ))),
            "std::env::names" => {
                let mut names: Vec<Value> = std::env::vars_os()
                    .map(|(key, _)| Value::String(key.to_string_lossy().into_owned()))
                    .collect();
                names.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
                Ok(Value::Array(names))
            }

            // -------------------------------------------------- std::fs
            "std::fs::exists" => Ok(Value::Bool(Path::new(&arg_str(args, 0, name)?).exists())),
            "std::fs::exists_case_exact" => {
                Ok(Value::Bool(exists_case_exact(&arg_str(args, 0, name)?)))
            }
            "std::fs::read_to_string" => {
                let path = arg_str(args, 0, name)?;
                let bytes = self.read_capped(&path, name)?;
                Ok(Value::String(String::from_utf8(bytes).map_err(|_| {
                    Error::Host(format!("{name}: {path} is not UTF-8"))
                })?))
            }
            "std::fs::read" => {
                let path = arg_str(args, 0, name)?;
                Ok(Value::Bytes(self.read_capped(&path, name)?))
            }
            "std::fs::write" => {
                fs::write(arg_str(args, 0, name)?, arg_str(args, 1, name)?)
                    .map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::write_bytes" => {
                fs::write(arg_str(args, 0, name)?, arg_bytes(args, 1, name)?)
                    .map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::copy" => {
                fs::copy(arg_str(args, 0, name)?, arg_str(args, 1, name)?)
                    .map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::rename" => {
                fs::rename(arg_str(args, 0, name)?, arg_str(args, 1, name)?)
                    .map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::create_dir" => {
                fs::create_dir(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::create_dir_all" => {
                fs::create_dir_all(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::remove_file" => {
                fs::remove_file(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::remove_dir" => {
                fs::remove_dir(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            "std::fs::remove_dir_all" => {
                fs::remove_dir_all(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(Value::Unit)
            }
            // The `try_*` family reports success as a bool instead of failing.
            "std::fs::try_remove_file" => Ok(Value::Bool(
                fs::remove_file(arg_str(args, 0, name)?).is_ok(),
            )),
            "std::fs::try_remove_dir_all" => Ok(Value::Bool(
                fs::remove_dir_all(arg_str(args, 0, name)?).is_ok(),
            )),
            "std::fs::try_copy" => Ok(Value::Bool(
                fs::copy(arg_str(args, 0, name)?, arg_str(args, 1, name)?).is_ok(),
            )),
            "std::fs::try_create_dir_all" => Ok(Value::Bool(
                fs::create_dir_all(arg_str(args, 0, name)?).is_ok(),
            )),
            "std::fs::try_rename" => Ok(Value::Bool(
                fs::rename(arg_str(args, 0, name)?, arg_str(args, 1, name)?).is_ok(),
            )),
            "std::fs::metadata" => {
                let meta = fs::metadata(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(self.store(Obj::Metadata { meta }, TY_METADATA))
            }
            "std::fs::symlink_metadata" => {
                let meta =
                    fs::symlink_metadata(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(self.store(Obj::Metadata { meta }, TY_METADATA))
            }
            "std::fs::read_dir" => {
                let dir = arg_str(args, 0, name)?;
                let mut entries: Vec<(PathBuf, String)> = fs::read_dir(&dir)
                    .map_err(|e| io_err(name, e))?
                    .filter_map(Result::ok)
                    .map(|entry| {
                        (
                            entry.path(),
                            entry.file_name().to_string_lossy().into_owned(),
                        )
                    })
                    .collect();
                // `read_dir` order is filesystem-defined; sort so scripts are
                // reproducible across platforms.
                entries.sort_by(|a, b| a.1.cmp(&b.1));
                let values = entries
                    .into_iter()
                    .map(|(path, name)| self.store(Obj::DirEntry { path, name }, TY_DIRENTRY))
                    .collect();
                Ok(Value::Array(values))
            }
            "std::fs::try_lock_exclusive" => {
                let path = arg_str(args, 0, name)?;
                match fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .write(true)
                    .open(&path)
                {
                    Ok(file) => Ok(self.store(Obj::FileLock(file), TY_FILELOCK)),
                    Err(error) => Err(io_err(name, error)),
                }
            }

            // ------------------------------------------------ std::path
            "std::path::PathBuf::from" => {
                let path = PathBuf::from(arg_str(args, 0, name)?);
                Ok(self.store(Obj::Path(path), TY_PATH))
            }
            "std::path::absolute" => {
                let path =
                    std::path::absolute(arg_str(args, 0, name)?).map_err(|e| io_err(name, e))?;
                Ok(self.store(Obj::Path(path), TY_PATH))
            }
            "std::path::join" => {
                let base = PathBuf::from(arg_str(args, 0, name)?);
                let joined = base.join(arg_str(args, 1, name)?);
                Ok(self.store(Obj::Path(joined), TY_PATH))
            }
            "std::path::parent" => {
                let path = PathBuf::from(arg_str(args, 0, name)?);
                let parent = path.parent().unwrap_or(Path::new("")).to_path_buf();
                Ok(self.store(Obj::Path(parent), TY_PATH))
            }

            // ------------------------------------------------ std::time
            "std::time::SystemTime::now" => {
                Ok(self.store(Obj::SystemTime(SystemTime::now()), TY_SYSTEMTIME))
            }
            "std::time::Duration::from_millis" => {
                let millis = arg_int(args, 0, name)?.max(0) as u64;
                Ok(self.store(Obj::Duration(Duration::from_millis(millis)), TY_DURATION))
            }
            "std::time::Duration::from_secs" => {
                let secs = arg_int(args, 0, name)?.max(0) as u64;
                Ok(self.store(Obj::Duration(Duration::from_secs(secs)), TY_DURATION))
            }

            // -------------------------------------------------- rh::* --
            "rh::bytes::from_text" => Ok(Value::Bytes(arg_str(args, 0, name)?.into_bytes())),
            "rh::bytes::from_array" => {
                match arg(args, 0, name)? {
                    Value::Array(items) => {
                        let mut bytes = Vec::with_capacity(items.len());
                        for item in items {
                            let byte = item
                                .as_int()
                                .ok_or_else(|| Error::Host(format!("{name}: expected ints")))?;
                            bytes.push(u8::try_from(byte).map_err(|_| {
                                Error::Host(format!("{name}: {byte} is not a byte"))
                            })?);
                        }
                        Ok(Value::Bytes(bytes))
                    }
                    other => Err(Error::Host(format!(
                        "{name}: expected an array, got {}",
                        other.type_name()
                    ))),
                }
            }
            "rh::crypto::sha256" => {
                use sha2::Digest;
                let mut hasher = sha2::Sha256::new();
                hasher.update(arg_bytes(args, 0, name)?);
                Ok(Value::String(hex(&hasher.finalize())))
            }
            "rh::crypto::sha256_file" => {
                use sha2::Digest;
                let path = arg_str(args, 0, name)?;
                let mut hasher = sha2::Sha256::new();
                hasher.update(self.read_capped(&path, name)?);
                Ok(Value::String(hex(&hasher.finalize())))
            }
            "rh::hash::fnv1a64" => {
                let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
                for byte in arg_bytes(args, 0, name)? {
                    hash ^= u64::from(byte);
                    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
                }
                Ok(Value::Int(hash as i64))
            }
            "rh::json::parse" => {
                let text = arg_str(args, 0, name)?;
                let json: serde_json::Value =
                    serde_json::from_str(&text).map_err(|e| Error::Host(format!("{name}: {e}")))?;
                Ok(json_to_value(&json))
            }
            "rh::json::parse_file" => {
                let path = arg_str(args, 0, name)?;
                let bytes = self.read_capped(&path, name)?;
                let json: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| Error::Host(format!("{name}: {e}")))?;
                Ok(json_to_value(&json))
            }
            "rh::json::stringify" => Ok(Value::String(
                serde_json::to_string(&value_to_json(arg(args, 0, name)?)?)
                    .map_err(|e| Error::Host(format!("{name}: {e}")))?,
            )),
            "rh::json::stringify_pretty" => Ok(Value::String(
                serde_json::to_string_pretty(&value_to_json(arg(args, 0, name)?)?)
                    .map_err(|e| Error::Host(format!("{name}: {e}")))?,
            )),
            "rh::runtime::temp_dir" => {
                Ok(Value::String(std::env::temp_dir().display().to_string()))
            }
            "rh::runtime::atomic_write" => {
                atomic_write(
                    &arg_str(args, 0, name)?,
                    arg_str(args, 1, name)?.as_bytes(),
                    name,
                )?;
                Ok(Value::Unit)
            }
            "rh::runtime::atomic_write_bytes" => {
                atomic_write(&arg_str(args, 0, name)?, &arg_bytes(args, 1, name)?, name)?;
                Ok(Value::Unit)
            }

            // ----------------------------------------------- std::process
            _ if name.starts_with("std::process::") => self.process_call(name, args),

            // ----------------------------- host object members ----------
            // Only a call whose receiver is one of *our* host objects is a
            // member call. A dot-form name with no host receiver is a foreign
            // surface (`fleet.tabs.list`) and is refused by name — AgenTerm's
            // adapter is what answers those.
            _ if name.contains('.') && matches!(args.first(), Some(Value::Host(_))) => {
                self.member_call(name, args)
            }

            _ => Err(Error::unsupported_name(name)),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn atomic_write(path: &str, bytes: &[u8], name: &str) -> Result<(), Error> {
    let target = Path::new(path);
    let temp = target.with_extension(format!("rh-tmp-{}", std::process::id()));
    fs::write(&temp, bytes).map_err(|e| io_err(name, e))?;
    fs::rename(&temp, target).map_err(|e| io_err(name, e))?;
    Ok(())
}

/// `exists` that also requires the on-disk spelling to match, for
/// case-insensitive filesystems.
fn exists_case_exact(path: &str) -> bool {
    let path = Path::new(path);
    if !path.exists() {
        return false;
    }
    let Some(file_name) = path.file_name() else {
        return true;
    };
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let parent = parent.unwrap_or(Path::new("."));
    match fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == file_name),
        Err(_) => false,
    }
}

// -------------------------------------------------------- capped file read

impl StdHost {
    /// Read a file, refusing anything over `fs_read_cap` (16 MiB by default,
    /// `RH_HOST_FS_READ_CAP`). The cap is checked against the file's declared
    /// length *before* allocating.
    fn read_capped(&self, path: &str, name: &str) -> Result<Vec<u8>, Error> {
        let meta = fs::metadata(path).map_err(|e| io_err(name, e))?;
        let len = usize::try_from(meta.len()).unwrap_or(usize::MAX);
        if len > self.fs_read_cap {
            return Err(Error::Host(format!(
                "{name}: {path} is {len} bytes, over the {} byte cap",
                self.fs_read_cap
            )));
        }
        let bytes = fs::read(path).map_err(|e| io_err(name, e))?;
        if bytes.len() > self.fs_read_cap {
            return Err(Error::Host(format!(
                "{name}: {path} grew past the {} byte cap",
                self.fs_read_cap
            )));
        }
        Ok(bytes)
    }
}

// ------------------------------------------------------------- std::process

impl StdHost {
    fn process_call(&mut self, name: &str, args: &[Value]) -> Result<Value, Error> {
        match name {
            "std::process::command" => {
                let spec = CommandSpec {
                    program: arg_str(args, 0, name)?,
                    capture_limit: 1 << 20,
                    ..CommandSpec::default()
                };
                Ok(self.store(Obj::Command(spec), TY_COMMAND))
            }
            "std::process::id" => Ok(Value::Int(i64::from(std::process::id()))),
            "std::process::kill" => {
                let pid = arg_int(args, 0, name)?;
                Ok(Value::Bool(kill_pid(pid)))
            }
            "std::process::command_status" => {
                let request = process_request(args, name)?;
                request.validate()?;
                let output = run_request(&request, name)?;
                Ok(Value::Int(i64::from(output.status.unwrap_or(-1))))
            }
            "std::process::command_stdout_file" => {
                let mut request = process_request(args, name)?;
                request.stdout_path = Some(arg_str(args, 2, name)?);
                request.validate()?;
                let output = run_request(&request, name)?;
                Ok(Value::Int(i64::from(output.status.unwrap_or(-1))))
            }
            // Enumerating processes needs platform APIs that `std` does not
            // expose. Out of reach for a std-only host; AgenTerm's Host has it.
            "std::process::list" => Err(Error::unsupported_name(name)),
            _ => Err(Error::unsupported_name(name)),
        }
    }
}

/// Build a `ProcessRequest` from `(program, args_array)`.
fn process_request(args: &[Value], name: &str) -> Result<ProcessRequest, Error> {
    let program = arg_str(args, 0, name)?;
    let mut list = Vec::new();
    if let Some(Value::Array(items)) = args.get(1) {
        for item in items {
            list.push(match item {
                Value::String(text) => text.clone(),
                other => {
                    return Err(Error::Host(format!(
                        "{name}: argument list must be strings, got {}",
                        other.type_name()
                    )));
                }
            });
        }
    }
    Ok(ProcessRequest {
        program,
        args: list,
        ..ProcessRequest::default()
    })
}

fn spec_to_request(spec: &CommandSpec) -> ProcessRequest {
    ProcessRequest {
        program: spec.program.clone(),
        args: spec.args.clone(),
        timeout_ms: spec.timeout_ms,
        stdout_path: spec.stdout_file.clone(),
        current_dir: spec.current_dir.clone(),
        env: spec.env.clone(),
        env_remove: spec.env_remove.clone(),
        env_clear: spec.env_clear,
    }
}

fn build_command(request: &ProcessRequest) -> Command {
    let mut command = Command::new(&request.program);
    command.args(&request.args);
    if let Some(dir) = &request.current_dir {
        command.current_dir(dir);
    }
    if request.env_clear {
        command.env_clear();
    }
    for key in &request.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &request.env {
        command.env(key, value);
    }
    command
}

fn run_request(request: &ProcessRequest, name: &str) -> Result<OutputData, Error> {
    let mut command = build_command(request);
    match &request.stdout_path {
        Some(path) => {
            let file = fs::File::create(path).map_err(|e| io_err(name, e))?;
            command.stdout(Stdio::from(file));
        }
        None => {
            command.stdout(Stdio::piped());
        }
    }
    command.stderr(Stdio::piped());
    let output = command.output().map_err(|e| io_err(name, e))?;
    Ok(OutputData {
        status: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
        truncated: false,
        error: None,
    })
}

/// Best-effort signal. Returns whether the process was there to kill.
fn kill_pid(pid: i64) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: `kill` with SIGTERM is a plain syscall; no memory is shared.
        let result = unsafe { libc_kill(pid as i32, 15) };
        result == 0
    }
    #[cfg(not(unix))]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
}

// -------------------------------------------- host object member dispatch

impl StdHost {
    /// `Type.member` calls. `args[0]` is always the receiver.
    ///
    /// `FileLock` and `Duration` appear in the value model but have **no**
    /// members: a lock is held for its binding's scope and a duration is only
    /// ever passed to `Command.timeout`.
    fn member_call(&mut self, name: &str, args: &[Value]) -> Result<Value, Error> {
        let receiver = arg(args, 0, name)?.clone();
        let rest = &args[1..];
        let (_, member) = name
            .split_once('.')
            .ok_or_else(|| Error::unsupported_name(name))?;

        match self.get(&receiver)? {
            Obj::Metadata { meta } => {
                let meta = meta.clone();
                return metadata_member(self, &meta, member, name);
            }
            Obj::DirEntry { path, name: file } => {
                let (path, file) = (path.clone(), file.clone());
                return self.direntry_member(&path, &file, member, name);
            }
            Obj::Path(path) => {
                let path = path.clone();
                return self.path_member(&path, member, rest, name);
            }
            Obj::SystemTime(time) => {
                let time = *time;
                return Ok(match member {
                    "rfc3339" => Value::String(rfc3339(time)),
                    "unix_millis" => Value::Int(unix_millis(time)),
                    _ => return Err(Error::unsupported_name(name)),
                });
            }
            Obj::Output(output) => {
                let output = output.clone();
                return output_member(&output, member, name);
            }
            Obj::Duration(_) | Obj::FileLock(_) => {
                return Err(Error::unsupported_name(name));
            }
            Obj::Command(_) | Obj::Child(_) => {}
        }

        // Command and Child mutate, so they are handled with `&mut self`.
        if matches!(self.get(&receiver)?, Obj::Command(_)) {
            return self.command_member(&receiver, member, rest, name);
        }
        self.child_member(&receiver, member, name)
    }

    fn direntry_member(
        &mut self,
        path: &Path,
        file_name: &str,
        member: &str,
        name: &str,
    ) -> Result<Value, Error> {
        Ok(match member {
            "file_name" => Value::String(file_name.to_owned()),
            "path" => self.store(Obj::Path(path.to_path_buf()), TY_PATH),
            "metadata" => {
                let meta = fs::metadata(path).map_err(|e| io_err(name, e))?;
                self.store(Obj::Metadata { meta }, TY_METADATA)
            }
            "is_file" => Value::Bool(path.is_file()),
            "is_dir" => Value::Bool(path.is_dir()),
            "is_symlink" => Value::Bool(
                fs::symlink_metadata(path)
                    .map(|meta| meta.file_type().is_symlink())
                    .unwrap_or(false),
            ),
            _ => return Err(Error::unsupported_name(name)),
        })
    }

    fn path_member(
        &mut self,
        path: &Path,
        member: &str,
        rest: &[Value],
        name: &str,
    ) -> Result<Value, Error> {
        Ok(match member {
            "display" => Value::String(path.display().to_string()),
            "file_name" => Value::String(
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            "extension" => Value::String(
                path.extension()
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            "is_absolute" => Value::Bool(path.is_absolute()),
            "join" => {
                let joined = path.join(arg_str(rest, 0, name)?);
                self.store(Obj::Path(joined), TY_PATH)
            }
            _ => return Err(Error::unsupported_name(name)),
        })
    }

    fn command_member(
        &mut self,
        receiver: &Value,
        member: &str,
        rest: &[Value],
        name: &str,
    ) -> Result<Value, Error> {
        // Builder methods mutate the stored spec and hand back the same
        // handle, so `command.arg("x")` works as a statement.
        match member {
            "arg" | "current_dir" | "stdin_text" | "stdin_bytes" | "stdout_file"
            | "stderr_file" | "timeout" | "capture_limit" | "env_clear" | "env_remove" | "env"
            | "args" => {
                let value = match member {
                    "arg" | "current_dir" | "stdout_file" | "stderr_file" | "env_remove" => {
                        Some(arg_str(rest, 0, name)?)
                    }
                    _ => None,
                };
                let bytes = if member == "stdin_bytes" || member == "stdin_text" {
                    Some(arg_bytes(rest, 0, name)?)
                } else {
                    None
                };
                let millis = if member == "timeout" {
                    Some(self.duration_millis(rest.first(), name)?)
                } else {
                    None
                };
                let list = if member == "args" {
                    match rest.first() {
                        Some(Value::Array(items)) => Some(
                            items
                                .iter()
                                .map(|item| match item {
                                    Value::String(text) => Ok(text.clone()),
                                    other => Err(Error::Host(format!(
                                        "{name}: expected strings, got {}",
                                        other.type_name()
                                    ))),
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        ),
                        _ => return Err(Error::Host(format!("{name}: expected an array"))),
                    }
                } else {
                    None
                };
                let pair = if member == "env" {
                    Some((arg_str(rest, 0, name)?, arg_str(rest, 1, name)?))
                } else {
                    None
                };
                let limit = if member == "capture_limit" {
                    Some(arg_int(rest, 0, name)?.max(0) as usize)
                } else {
                    None
                };

                let Obj::Command(spec) = self.get_mut(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Command")));
                };
                match member {
                    "arg" => spec.args.push(value.unwrap_or_default()),
                    "args" => spec.args.extend(list.unwrap_or_default()),
                    "current_dir" => spec.current_dir = value,
                    "stdout_file" => spec.stdout_file = value,
                    "stderr_file" => spec.stderr_file = value,
                    "stdin_text" | "stdin_bytes" => spec.stdin_bytes = bytes,
                    "timeout" => spec.timeout_ms = millis.unwrap_or(0),
                    "capture_limit" => spec.capture_limit = limit.unwrap_or(0),
                    "env_clear" => spec.env_clear = true,
                    "env_remove" => spec.env_remove.push(value.unwrap_or_default()),
                    "env" => {
                        if let Some(pair) = pair {
                            spec.env.push(pair);
                        }
                    }
                    _ => unreachable!("member is in the matched set"),
                }
                Ok(receiver.clone())
            }
            "output" => {
                let Obj::Command(spec) = self.get(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Command")));
                };
                let request = spec_to_request(spec);
                request.validate()?;
                let output = run_request(&request, name)?;
                Ok(self.store(Obj::Output(output), TY_OUTPUT))
            }
            "start" => {
                let Obj::Command(spec) = self.get(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Command")));
                };
                let request = spec_to_request(spec);
                request.validate()?;
                let mut command = build_command(&request);
                command.stdout(Stdio::piped()).stderr(Stdio::piped());
                let child = command.spawn().map_err(|e| io_err(name, e))?;
                Ok(self.store(Obj::Child(Some(child)), TY_CHILD))
            }
            _ => Err(Error::unsupported_name(name)),
        }
    }

    fn duration_millis(&self, value: Option<&Value>, name: &str) -> Result<u64, Error> {
        match value {
            Some(Value::Int(millis)) => Ok((*millis).max(0) as u64),
            Some(host @ Value::Host(_)) => match self.get(host)? {
                Obj::Duration(duration) => Ok(duration.as_millis() as u64),
                _ => Err(Error::Host(format!("{name}: expected a Duration"))),
            },
            _ => Err(Error::Host(format!("{name}: expected a Duration"))),
        }
    }

    fn child_member(&mut self, receiver: &Value, member: &str, name: &str) -> Result<Value, Error> {
        match member {
            "id" => {
                let Obj::Child(child) = self.get(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Child")));
                };
                Ok(Value::Int(child.as_ref().map_or(-1, |c| i64::from(c.id()))))
            }
            "state" => {
                let Obj::Child(child) = self.get_mut(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Child")));
                };
                let state = match child.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(_)) => "exited",
                        Ok(None) => "running",
                        Err(_) => "unknown",
                    },
                    None => "exited",
                };
                Ok(Value::String(state.to_owned()))
            }
            // NOTE: live `Child.stdout` yields a `Stream` handle, but `Stream`
            // is explicitly **not** Language 1 (value-model table). Language 1
            // therefore reads the pipe to completion and returns the bytes.
            "stdout" | "stderr" => {
                let Obj::Child(child) = self.get_mut(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Child")));
                };
                let Some(child) = child.as_mut() else {
                    return Ok(Value::Bytes(Vec::new()));
                };
                let mut buffer = Vec::new();
                if member == "stdout" {
                    if let Some(pipe) = child.stdout.as_mut() {
                        pipe.read_to_end(&mut buffer).map_err(|e| io_err(name, e))?;
                    }
                } else if let Some(pipe) = child.stderr.as_mut() {
                    pipe.read_to_end(&mut buffer).map_err(|e| io_err(name, e))?;
                }
                Ok(Value::Bytes(buffer))
            }
            "kill" | "kill_tree" => {
                let Obj::Child(child) = self.get_mut(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Child")));
                };
                match child.as_mut() {
                    Some(child) => Ok(Value::Bool(child.kill().is_ok())),
                    None => Ok(Value::Bool(false)),
                }
            }
            "wait_with_output" => {
                let Obj::Child(slot) = self.get_mut(receiver)? else {
                    return Err(Error::Host(format!("{name}: not a Child")));
                };
                let Some(child) = slot.take() else {
                    return Err(Error::Host(format!("{name}: child already waited on")));
                };
                let output = child.wait_with_output().map_err(|e| io_err(name, e))?;
                let data = OutputData {
                    status: output.status.code(),
                    stdout: output.stdout,
                    stderr: output.stderr,
                    truncated: false,
                    error: None,
                };
                Ok(self.store(Obj::Output(data), TY_OUTPUT))
            }
            _ => Err(Error::unsupported_name(name)),
        }
    }
}

fn metadata_member(
    host: &mut StdHost,
    meta: &fs::Metadata,
    member: &str,
    name: &str,
) -> Result<Value, Error> {
    Ok(match member {
        "is_file" => Value::Bool(meta.is_file()),
        "is_dir" => Value::Bool(meta.is_dir()),
        "is_symlink" => Value::Bool(meta.file_type().is_symlink()),
        "is_reparse_point" => Value::Bool(meta.file_type().is_symlink()),
        "len" => Value::Int(i64::try_from(meta.len()).unwrap_or(i64::MAX)),
        "modified" => {
            let time = meta.modified().map_err(|e| io_err(name, e))?;
            host.store(Obj::SystemTime(time), TY_SYSTEMTIME)
        }
        _ => return Err(Error::unsupported_name(name)),
    })
}

fn output_member(output: &OutputData, member: &str, name: &str) -> Result<Value, Error> {
    Ok(match member {
        "success" => Value::Bool(output.status == Some(0)),
        "exit_code" => Value::Int(i64::from(output.status.unwrap_or(-1))),
        "stdout" => Value::Bytes(output.stdout.clone()),
        "stderr" => Value::Bytes(output.stderr.clone()),
        "stdout_text" => Value::String(String::from_utf8_lossy(&output.stdout).into_owned()),
        "stderr_text" => Value::String(String::from_utf8_lossy(&output.stderr).into_owned()),
        "combined_text" => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            Value::String(text)
        }
        "complete" => Value::Bool(!output.truncated),
        "truncated" => Value::Bool(output.truncated),
        "error" => Value::String(output.error.clone().unwrap_or_default()),
        "require_success" => {
            if output.status == Some(0) {
                Value::Unit
            } else {
                return Err(Error::Host(format!(
                    "{name}: command failed with {:?}",
                    output.status
                )));
            }
        }
        _ => return Err(Error::unsupported_name(name)),
    })
}
