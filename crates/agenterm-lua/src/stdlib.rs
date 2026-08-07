//! Standard library for Lua scripts: `std.fs`, `std.path`, `std.env`, `std.time`, etc.
//! Aligned with rh's shipped_surfaces API surface.

use std::io::Read;
use std::path::Path;

use mlua::{Lua, LuaSerdeExt, Table, Value};

/// Inject the full `std` global table into the Lua runtime.
pub fn inject(lua: &Lua) -> Result<(), mlua::Error> {
    let std_table = lua.create_table()?;
    std_table.set("fs", build_fs(lua)?)?;
    std_table.set("process", build_process(lua)?)?;
    std_table.set("path", build_path(lua)?)?;
    std_table.set("env", build_env(lua)?)?;
    std_table.set("time", build_time(lua)?)?;
    std_table.set("json", build_json(lua)?)?;
    std_table.set("crypto", build_crypto(lua)?)?;
    lua.globals().set("std", std_table)?;

    // rhai.runtime table (compatibility alias for rh's atomic_write etc.)
    let rhai_table = lua.create_table()?;
    let runtime_table = lua.create_table()?;
    runtime_table.set(
        "atomic_write",
        lua.create_function(|_lua, (path, content): (String, String)| {
            atomic_write(&path, &content)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("atomic_write: {e}")))
        })?,
    )?;
    rhai_table.set("runtime", runtime_table)?;

    // rhai::hash table
    let hash_table = lua.create_table()?;
    hash_table.set(
        "fnv1a64",
        lua.create_function(|_lua, data: String| Ok(fnv1a64(&data)))?,
    )?;
    rhai_table.set("hash", hash_table)?;

    lua.globals().set("rhai", rhai_table)?;

    // rh::fail global
    lua.globals().set(
        "rh",
        lua.create_table_from([(
            "fail",
            lua.create_function(|_lua, msg: String| -> Result<(), mlua::Error> {
                Err(mlua::Error::runtime(format!("rh_fail: {msg}")))
            })?,
        )])?,
    )?;

    Ok(())
}

/// FNV-1a 64-bit hash, returns lowercase hex string.
fn fnv1a64(data: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in data.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Atomic file write: write to temp file, then rename (on same filesystem).
fn atomic_write(path: &str, content: &str) -> Result<(), std::io::Error> {
    let path = std::path::Path::new(path);
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, content.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

// ── helpers ─────────────────────────────────────────────────────────────

/// Extract a sequence of strings from a Lua table (1-indexed array).
fn table_to_strings(table: &Table) -> Result<Vec<String>, mlua::Error> {
    let mut out = Vec::new();
    let len = table.raw_len();
    for i in 1..=len {
        let val: Value = table.raw_get(i)?;
        if let Value::String(s) = val {
            out.push(s.to_str()?.to_string());
        }
    }
    Ok(out)
}

fn read_to_string(pipe: Option<impl std::io::Read>) -> String {
    if let Some(mut p) = pipe {
        let mut buf = Vec::new();
        let _ = p.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    }
}

/// Spawn a process, capture stdout + stderr, return (success, exit_code, stdout, stderr).
fn spawn_and_capture(
    program: &str,
    args: &[String],
    timeout_ms: u64,
) -> Result<(bool, i32, String, String), mlua::Error> {
    use std::process::{Command, Stdio};

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| mlua::Error::runtime(format!("process_spawn: {e}")))?;

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    let exit_code = wait_child(&mut child, deadline);

    let stdout = read_to_string(stdout_pipe);
    let stderr = read_to_string(stderr_pipe);
    let success = exit_code == 0;

    Ok((success, exit_code, stdout, stderr))
}

/// Spawn a process, write stdout to file, capture stderr.
fn spawn_stdout_file(
    program: &str,
    args: &[String],
    stdout_path: &str,
    timeout_ms: u64,
) -> Result<(bool, i32, String), mlua::Error> {
    use std::process::{Command, Stdio};

    let file = std::fs::File::create(stdout_path)
        .map_err(|e| mlua::Error::runtime(format!("process_stdout_file: {e}")))?;

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.stdin(Stdio::null());
    cmd.stdout(file);
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| mlua::Error::runtime(format!("process_spawn: {e}")))?;

    let stderr_pipe = child.stderr.take();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    let exit_code = wait_child(&mut child, deadline);

    let stderr = if let Some(mut p) = stderr_pipe {
        let mut buf = Vec::new();
        let _ = p.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    } else {
        String::new()
    };

    Ok((exit_code == 0, exit_code, stderr))
}

fn wait_child(child: &mut std::process::Child, deadline: std::time::Instant) -> i32 {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code().unwrap_or(-1),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return -1;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return -1;
            }
        }
    }
}

// ── std.fs ──────────────────────────────────────────────────────────────

fn build_fs(lua: &Lua) -> Result<Table, mlua::Error> {
    let fs = lua.create_table()?;

    // std.fs.exists(path) → bool
    fs.set(
        "exists",
        lua.create_function(|_lua, path: String| Ok(Path::new(&path).exists()))?,
    )?;

    // std.fs.read(path) → string | nil, err
    fs.set(
        "read",
        lua.create_function(|_lua, path: String| {
            std::fs::read(&path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| mlua::Error::runtime(format!("fs_read: {e}")))
        })?,
    )?;

    // std.fs.write(path, content) → true | nil, err
    fs.set(
        "write",
        lua.create_function(|_lua, (path, content): (String, String)| {
            std::fs::write(&path, &content)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_write: {e}")))
        })?,
    )?;

    // std.fs.metadata(path) → {is_file, is_dir, is_symlink, len, modified}
    fs.set(
        "metadata",
        lua.create_function(|lua, path: String| {
            let meta = std::fs::metadata(&path)
                .map_err(|e| mlua::Error::runtime(format!("fs_metadata: {e}")))?;
            let table = lua.create_table()?;
            table.set("is_file", meta.is_file())?;
            table.set("is_dir", meta.is_dir())?;
            table.set("is_symlink", meta.file_type().is_symlink())?;
            table.set("len", meta.len() as i64)?;
            let modified: Option<i64> = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok());
            table.set("modified", modified.unwrap_or(0))?;
            Ok(table)
        })?,
    )?;

    // std.fs.copy(src, dst) → true | nil, err
    fs.set(
        "copy",
        lua.create_function(|_lua, (src, dst): (String, String)| {
            std::fs::copy(&src, &dst)
                .map(|_| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_copy: {e}")))
        })?,
    )?;

    // std.fs.create_dir(path) → true | nil, err  (alias for create_dir_all)
    fs.set(
        "create_dir",
        lua.create_function(|_lua, path: String| {
            std::fs::create_dir_all(&path)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_create_dir: {e}")))
        })?,
    )?;

    // std.fs.read_dir(path) → array of {name, is_file, is_dir, path}
    fs.set(
        "read_dir",
        lua.create_function(|lua, path: String| {
            let entries: Vec<serde_json::Value> = std::fs::read_dir(&path)
                .map_err(|e| mlua::Error::runtime(format!("fs_read_dir: {e}")))?
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let ft = entry.file_type().ok()?;
                    Some(serde_json::json!({
                        "name": entry.file_name().to_string_lossy(),
                        "is_file": ft.is_file(),
                        "is_dir": ft.is_dir(),
                        "path": entry.path().to_string_lossy().into_owned(),
                    }))
                })
                .collect();
            lua.to_value(&entries)
        })?,
    )?;

    // std.fs.remove_file(path) → true | nil, err
    fs.set(
        "remove_file",
        lua.create_function(|_lua, path: String| {
            std::fs::remove_file(&path)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_remove_file: {e}")))
        })?,
    )?;

    // std.fs.rename(src, dst) → true | nil, err
    fs.set(
        "rename",
        lua.create_function(|_lua, (src, dst): (String, String)| {
            std::fs::rename(&src, &dst)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_rename: {e}")))
        })?,
    )?;

    // std.fs.remove_dir_all(path) → true | nil, err
    fs.set(
        "remove_dir_all",
        lua.create_function(|_lua, path: String| {
            std::fs::remove_dir_all(&path)
                .map(|()| true)
                .map_err(|e| mlua::Error::runtime(format!("fs_remove_dir_all: {e}")))
        })?,
    )?;

    // std.fs.symlink_metadata(path) → {is_file, is_dir, is_symlink, len}
    fs.set(
        "symlink_metadata",
        lua.create_function(|lua, path: String| {
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|e| mlua::Error::runtime(format!("fs_symlink_metadata: {e}")))?;
            let table = lua.create_table()?;
            table.set("is_file", meta.is_file())?;
            table.set("is_dir", meta.is_dir())?;
            table.set("is_symlink", meta.file_type().is_symlink())?;
            table.set("len", meta.len() as i64)?;
            Ok(table)
        })?,
    )?;

    Ok(fs)
}

// ── std.process ─────────────────────────────────────────────────────────

fn build_process(lua: &Lua) -> Result<Table, mlua::Error> {
    let process = lua.create_table()?;

    // std.process.command(program, args, timeout_ms) → {success, exit_code, stdout, stderr}
    process.set(
        "command",
        lua.create_function(|lua, (program, args_tbl, timeout_ms): (String, Table, Option<u64>)| {
            let args = table_to_strings(&args_tbl)?;
            let timeout = timeout_ms.unwrap_or(30_000).clamp(1, 3_600_000);
            let (success, exit_code, stdout, stderr) =
                spawn_and_capture(&program, &args, timeout)?;
            let out = lua.create_table()?;
            out.set("success", success)?;
            out.set("exit_code", exit_code)?;
            out.set("stdout", stdout)?;
            out.set("stderr", stderr)?;
            Ok(out)
        })?,
    )?;

    // std.process.status(program, args, timeout_ms) → {success, exit_code}
    process.set(
        "status",
        lua.create_function(|lua, (program, args_tbl, timeout_ms): (String, Table, Option<u64>)| {
            let args = table_to_strings(&args_tbl)?;
            let timeout = timeout_ms.unwrap_or(30_000).clamp(1, 3_600_000);
            let (success, exit_code, _, _) = spawn_and_capture(&program, &args, timeout)?;
            let out = lua.create_table()?;
            out.set("success", success)?;
            out.set("exit_code", exit_code)?;
            Ok(out)
        })?,
    )?;

    // std.process.stdout_file(program, args, stdout_path, timeout_ms) → {success, exit_code}
    process.set(
        "stdout_file",
        lua.create_function(
            |lua, (program, args_tbl, stdout_path, timeout_ms): (String, Table, String, Option<u64>)| {
                let args = table_to_strings(&args_tbl)?;
                let timeout = timeout_ms.unwrap_or(30_000).clamp(1, 3_600_000);
                let (success, exit_code, _stderr) =
                    spawn_stdout_file(&program, &args, &stdout_path, timeout)?;
                let out = lua.create_table()?;
                out.set("success", success)?;
                out.set("exit_code", exit_code)?;
                Ok(out)
            },
        )?,
    )?;

    // std.process.id() → int
    process.set(
        "id",
        lua.create_function(|_lua, ()| Ok(std::process::id() as i64))?,
    )?;

    // std.process.list() → table of {name, pid}
    process.set(
        "list",
        lua.create_function(|lua, ()| {
            // Use tasklist on Windows, ps on Unix
            let (prog, args) = if cfg!(windows) {
                ("tasklist", vec!["/FO".to_string(), "CSV".to_string(), "/NH".to_string()])
            } else {
                ("ps", vec!["-eo".to_string(), "comm,pid".to_string(), "--no-headers".to_string()])
            };
            let output = std::process::Command::new(prog)
                .args(&args)
                .output()
                .map_err(|e| mlua::Error::runtime(format!("process_list: {e}")))?;
            let text = String::from_utf8_lossy(&output.stdout);
            let mut entries = Vec::new();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() { continue; }
                if cfg!(windows) {
                    // tasklist CSV: "name.exe","pid",...
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 2 {
                        let name = parts[0].trim_matches('"').trim().to_string();
                        if let Ok(pid) = parts[1].trim_matches('"').trim().parse::<i64>() {
                            entries.push(serde_json::json!({"name": name, "pid": pid}));
                        }
                    }
                } else {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[0].to_string();
                        if let Ok(pid) = parts[parts.len()-1].parse::<i64>() {
                            entries.push(serde_json::json!({"name": name, "pid": pid}));
                        }
                    }
                }
            }
            lua.to_value(&entries)
        })?,
    )?;

    Ok(process)
}

// ── std.path ────────────────────────────────────────────────────────────

fn build_path(lua: &Lua) -> Result<Table, mlua::Error> {
    let path = lua.create_table()?;

    // std.path.absolute(p) → string
    path.set(
        "absolute",
        lua.create_function(|_lua, p: String| {
            std::path::absolute(&p)
                .map(|abs| abs.to_string_lossy().into_owned())
                .map_err(|e| mlua::Error::runtime(format!("path_absolute: {e}")))
        })?,
    )?;

    // std.path.join(base, child) → string
    path.set(
        "join",
        lua.create_function(|_lua, (base, child): (String, String)| {
            let joined = Path::new(&base).join(&child);
            Ok(joined.to_string_lossy().into_owned())
        })?,
    )?;

    // std.path.parent(p) → string
    path.set(
        "parent",
        lua.create_function(|_lua, p: String| {
            let parent = Path::new(&p)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(parent)
        })?,
    )?;

    // std.path.file_name(p) → string
    path.set(
        "file_name",
        lua.create_function(|_lua, p: String| {
            let name = Path::new(&p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(name)
        })?,
    )?;

    // std.path.is_absolute(p) → bool
    path.set(
        "is_absolute",
        lua.create_function(|_lua, p: String| Ok(Path::new(&p).is_absolute()))?,
    )?;

    // std.path.from(parts...) → string (PathBuf::from equivalent)
    path.set(
        "from",
        lua.create_function(|_lua, parts: Vec<String>| {
            if parts.is_empty() {
                return Ok(String::new());
            }
            let mut p = std::path::PathBuf::from(&parts[0]);
            for part in &parts[1..] {
                p.push(part);
            }
            Ok(p.to_string_lossy().into_owned())
        })?,
    )?;

    Ok(path)
}

// ── std.env ─────────────────────────────────────────────────────────────

fn build_env(lua: &Lua) -> Result<Table, mlua::Error> {
    let env = lua.create_table()?;

    // std.env.get(name) → string | nil
    env.set(
        "get",
        lua.create_function(|_lua, name: String| Ok(std::env::var(&name).ok()))?,
    )?;

    // std.env.has(name) → bool
    env.set(
        "has",
        lua.create_function(|_lua, name: String| Ok(std::env::var_os(&name).is_some()))?,
    )?;

    // std.env.current_dir() → string
    env.set(
        "current_dir",
        lua.create_function(|_lua, ()| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .map_err(|e| mlua::Error::runtime(format!("env_current_dir: {e}")))
        })?,
    )?;

    Ok(env)
}

// ── std.time ────────────────────────────────────────────────────────────

fn build_time(lua: &Lua) -> Result<Table, mlua::Error> {
    let time = lua.create_table()?;

    // std.time.now_unix_ms() → int (millis since Unix epoch)
    time.set(
        "now_unix_ms",
        lua.create_function(|_lua, ()| {
            let dur = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| mlua::Error::runtime(format!("system_time: {e}")))?;
            let millis = i64::try_from(dur.as_millis()).unwrap_or(0);
            Ok(millis)
        })?,
    )?;

    // std.time.now_rfc3339() → string
    time.set(
        "now_rfc3339",
        lua.create_function(|_lua, ()| {
            let now = std::time::SystemTime::now();
            let dur = now
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| mlua::Error::runtime(format!("system_time: {e}")))?;
            let seconds = i64::try_from(dur.as_secs()).unwrap_or(0);
            let subsec = dur.subsec_millis();
            let days = seconds / 86_400;
            let day_seconds = seconds % 86_400;
            let (year, month, day) = civil_date_from_unix_days(days);
            let hour = day_seconds / 3_600;
            let minute = (day_seconds % 3_600) / 60;
            let second = day_seconds % 60;
            Ok(format!(
                "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{subsec:03}Z"
            ))
        })?,
    )?;

    // std.time.Duration namespace
    let duration = lua.create_table()?;
    duration.set(
        "from_millis",
        lua.create_function(|_lua, n: i64| Ok(n))?,
    )?;
    duration.set(
        "from_secs",
        lua.create_function(|_lua, n: i64| Ok(n * 1000))?,
    )?;
    time.set("Duration", duration)?;

    Ok(time)
}

/// Calendar date from Unix epoch days (port of Howard Hinnant's algorithm).
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

// ── std.json ────────────────────────────────────────────────────────────

fn build_json(lua: &Lua) -> Result<Table, mlua::Error> {
    let json = lua.create_table()?;

    // std.json.parse(s) → value (table/number/string/bool/nil)
    json.set(
        "parse",
        lua.create_function(|lua, s: String| {
            let v: serde_json::Value = serde_json::from_str(&s)
                .map_err(|e| mlua::Error::runtime(format!("json_parse: {e}")))?;
            lua.to_value(&v)
        })?,
    )?;

    // std.json.stringify(v) → string
    json.set(
        "stringify",
        lua.create_function(|_lua, val: Value| {
            let v: serde_json::Value = serde_json::to_value(&val)
                .map_err(|e| mlua::Error::runtime(format!("json_stringify: {e}")))?;
            // Use compact (non-pretty) output matching rh's default
            serde_json::to_string(&v)
                .map_err(|e| mlua::Error::runtime(format!("json_stringify: {e}")))
        })?,
    )?;

    Ok(json)
}

// ── std.crypto ──────────────────────────────────────────────────────────

fn build_crypto(lua: &Lua) -> Result<Table, mlua::Error> {
    use sha2::Digest;

    let crypto = lua.create_table()?;

    // std.crypto.sha256(data) → hex string
    crypto.set(
        "sha256",
        lua.create_function(|_lua, data: String| {
            let hash = sha2::Sha256::digest(data.as_bytes());
            Ok(hex_encode(&hash))
        })?,
    )?;

    // std.crypto.sha256_file(path) → hex string
    crypto.set(
        "sha256_file",
        lua.create_function(|_lua, path: String| {
            let bytes = std::fs::read(&path)
                .map_err(|e| mlua::Error::runtime(format!("sha256_file_read: {e}")))?;
            let hash = sha2::Sha256::digest(&bytes);
            Ok(hex_encode(&hash))
        })?,
    )?;

    Ok(crypto)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use tempfile::TempDir;

    fn engine() -> LuaEngine {
        LuaEngine::new().expect("engine")
    }

    fn host() -> LuaHostFunctions {
        LuaHostFunctions::default()
    }

    // ── std.fs ──────────────────────────────────────────────────────

    #[test]
    fn fs_exists_true() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("f1.txt");
        std::fs::write(&p, "hi").expect("write");
        let e = engine();
        let r = e
            .eval(
                &format!("return std.fs.exists([[{}]]) and 1 or 0", p.display()),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn fs_exists_false() {
        let e = engine();
        let r = e
            .eval("return std.fs.exists('/nonexistent_agenterm_test') and 1 or 0", &host())
            .expect("eval");
        assert_eq!(r.value, 0);
    }

    #[test]
    fn fs_read() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("content.txt");
        std::fs::write(&p, "hello lua").expect("write");
        let e = engine();
        let r = e
            .eval(
                &format!("local s = std.fs.read([[{}]]); return #s", p.display()),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 9, "hello lua = 9 chars");
    }

    #[test]
    fn fs_read_missing_errors() {
        let e = engine();
        let r = e.eval(
            "local ok, err = pcall(function() return std.fs.read('/nonexistent_agenterm_test') end); return ok and 0 or 1",
            &host(),
        );
        if let Ok(r) = r {
            assert_eq!(r.value, 1, "expected error for missing file");
        }
    }

    #[test]
    fn fs_write_and_read_roundtrip() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("rw.txt");
        let e = engine();
        e.eval(
            &format!("std.fs.write([[{}]], 'roundtrip')", p.display()),
            &host(),
        )
        .expect("write");
        let content = std::fs::read_to_string(&p).expect("read");
        assert_eq!(content, "roundtrip");
    }

    #[test]
    fn fs_metadata_file() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("meta.txt");
        std::fs::write(&p, "abc").expect("write");
        let e = engine();
        let r = e
            .eval(
                &format!(
                    "local m = std.fs.metadata([[{}]]); return m.is_file and 1 or 0",
                    p.display()
                ),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn fs_metadata_dir() {
        let dir = TempDir::new().expect("tempdir");
        let e = engine();
        let r = e
            .eval(
                &format!(
                    "local m = std.fs.metadata([[{}]]); return m.is_dir and 1 or 0",
                    dir.path().display()
                ),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn fs_metadata_len() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("len.txt");
        std::fs::write(&p, "12345").expect("write");
        let e = engine();
        let r = e
            .eval(
                &format!(
                    "local m = std.fs.metadata([[{}]]); return m.len",
                    p.display()
                ),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 5);
    }

    #[test]
    fn fs_copy() {
        let dir = TempDir::new().expect("tempdir");
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, "copied").expect("write");
        let e = engine();
        e.eval(
            &format!(
                "std.fs.copy([[{}]], [[{}]])",
                src.display(),
                dst.display()
            ),
            &host(),
        )
        .expect("copy");
        let content = std::fs::read_to_string(&dst).expect("read");
        assert_eq!(content, "copied");
    }

    #[test]
    fn fs_create_dir() {
        let dir = TempDir::new().expect("tempdir");
        let sub = dir.path().join("subdir");
        let e = engine();
        e.eval(
            &format!("std.fs.create_dir([[{}]])", sub.display()),
            &host(),
        )
        .expect("create_dir");
        assert!(sub.is_dir());
    }

    #[test]
    fn fs_create_dir_nested() {
        let dir = TempDir::new().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("c");
        let e = engine();
        e.eval(
            &format!("std.fs.create_dir([[{}]])", nested.display()),
            &host(),
        )
        .expect("create_dir");
        assert!(nested.is_dir());
    }

    // ── std.process ─────────────────────────────────────────────────

    #[test]
    fn process_command_echo() {
        let e = engine();
        let r = e
            .eval(
                "local out = std.process.command('cmd', {'/c', 'echo', 'hello'}, 5000); print(out.stdout); return out.exit_code",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 0);
        assert!(r.stdout.trim().contains("hello"));
    }

    #[test]
    fn process_command_exit_code() {
        let e = engine();
        let r = e
            .eval(
                "local out = std.process.command('cmd', {'/c', 'exit', '0'}, 5000); return out.exit_code",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 0);
    }

    #[test]
    fn process_status() {
        let e = engine();
        let r = e
            .eval(
                "local out = std.process.status('cmd', {'/c', 'echo', 'ok'}, 5000); return out.success and 1 or 0",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn process_stdout_file() {
        let dir = TempDir::new().expect("tempdir");
        let out_file = dir.path().join("out.txt");
        let e = engine();
        e.eval(
            &format!(
                "local r = std.process.stdout_file('cmd', {{'/c', 'echo', 'saved'}}, [[{}]], 5000); return r.exit_code",
                out_file.display()
            ),
            &host(),
        )
        .expect("eval");
        let content = std::fs::read_to_string(&out_file).expect("read");
        assert!(content.trim().contains("saved"));
    }

    // ── std.path ────────────────────────────────────────────────────

    #[test]
    fn path_join() {
        let e = engine();
        let r = e
            .eval(
                "local p = std.path.join('/foo', 'bar'); return #p",
                &host(),
            )
            .expect("eval");
        // "/foo/bar" or "/foo\\bar" on Windows
        assert!(r.value >= 7);
    }

    #[test]
    fn path_parent() {
        let e = engine();
        let r = e
            .eval(
                "local p = std.path.parent('/foo/bar/baz.txt'); return #p",
                &host(),
            )
            .expect("eval");
        assert!(r.value > 0);
    }

    #[test]
    fn path_file_name() {
        let e = engine();
        let r = e
            .eval(
                "print(std.path.file_name('/foo/bar/baz.txt')); return 1",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
        assert_eq!(r.stdout.trim(), "baz.txt");
    }

    #[test]
    fn path_is_absolute() {
        let e = engine();
        // On Windows absolute needs drive letter; on Unix "/" is absolute
        let abs_path = if cfg!(windows) { r"C:\" } else { "/" };
        let r = e
            .eval(
                &format!("return std.path.is_absolute([[{}]]) and 1 or 0", abs_path),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    // ── std.env ─────────────────────────────────────────────────────

    #[test]
    fn env_has_path() {
        let e = engine();
        let r = e
            .eval("return std.env.has('PATH') and 1 or 0", &host())
            .expect("eval");
        assert_eq!(r.value, 1, "PATH should always exist");
    }

    #[test]
    fn env_has_missing() {
        let e = engine();
        let r = e
            .eval("return std.env.has('AGENTERM_NONEXISTENT_VAR') and 1 or 0", &host())
            .expect("eval");
        assert_eq!(r.value, 0);
    }

    #[test]
    fn env_get_path() {
        let e = engine();
        let r = e
            .eval("return #(std.env.get('PATH') or '')", &host())
            .expect("eval");
        assert!(r.value > 0, "PATH should be non-empty");
    }

    #[test]
    fn env_get_missing_is_nil() {
        let e = engine();
        let r = e
            .eval(
                "local v = std.env.get('AGENTERM_NONEXISTENT_VAR'); return v == nil and 1 or 0",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn env_current_dir() {
        let e = engine();
        let r = e
            .eval("local d = std.env.current_dir(); return #d", &host())
            .expect("eval");
        assert!(r.value > 0);
    }

    // ── std.time ────────────────────────────────────────────────────

    #[test]
    fn time_now_unix_ms() {
        let e = engine();
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let r = e
            .eval("return std.time.now_unix_ms()", &host())
            .expect("eval");
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        assert!(r.value >= before, "{} >= {}", r.value, before);
        assert!(r.value <= after, "{} <= {}", r.value, after);
    }

    #[test]
    fn time_now_rfc3339() {
        let e = engine();
        let r = e
            .eval("local s = std.time.now_rfc3339(); print(s); return #s", &host())
            .expect("eval");
        assert_eq!(r.value, 24, "RFC3339 with ms = 24 chars");
        // Check format: YYYY-MM-DDTHH:MM:SS.sssZ
        let s = r.stdout.trim();
        assert!(s.contains('T'), "missing T separator");
        assert!(s.ends_with('Z'), "missing Z suffix");
    }

    // ── std.json ────────────────────────────────────────────────────

    #[test]
    fn json_parse_object() {
        let e = engine();
        let r = e
            .eval(
                "local obj = std.json.parse('{\"a\":1,\"b\":\"x\"}'); return obj.a",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn json_parse_array() {
        let e = engine();
        let r = e
            .eval(
                "local arr = std.json.parse('[10, 20, 30]'); return arr[1]",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 10);
    }

    #[test]
    fn json_stringify_table() {
        let e = engine();
        let r = e
            .eval(
                "local s = std.json.stringify({x = 1, y = 'hi'}); print(s); return string.find(s, '\"x\"') ~= nil and 1 or 0",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    #[test]
    fn json_roundtrip() {
        let e = engine();
        let r = e
            .eval(
                "local obj = std.json.parse('{\"key\":\"value\"}'); local s = std.json.stringify(obj); print(s); return string.find(s, '\"key\"') ~= nil and 1 or 0",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    // ── std.crypto ──────────────────────────────────────────────────

    #[test]
    fn crypto_sha256() {
        let e = engine();
        let r = e
            .eval(
                "local h = std.crypto.sha256('abc'); print(h); return #h",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 64);
        // Known SHA256 of "abc"
        assert_eq!(
            r.stdout.trim(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn crypto_sha256_empty() {
        let e = engine();
        let r = e
            .eval("return #std.crypto.sha256('')", &host())
            .expect("eval");
        assert_eq!(r.value, 64);
    }

    #[test]
    fn crypto_sha256_file() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("hashme.txt");
        std::fs::write(&p, "abc").expect("write");
        let e = engine();
        let r = e
            .eval(
                &format!(
                    "local h = std.crypto.sha256_file([[{}]]); print(h); return #h",
                    p.display()
                ),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 64);
        assert_eq!(
            r.stdout.trim(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // ── std.fs additions ────────────────────────────────────────────

    #[test]
    fn fs_read_dir_lists_entries() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let e = engine();
        let r = e
            .eval(
                &format!("local entries = std.fs.read_dir([[{}]]); return #entries", dir.path().display()),
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 3);
    }

    #[test]
    fn fs_remove_file_deletes() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("to_delete.txt");
        std::fs::write(&p, "x").unwrap();
        let e = engine();
        e.eval(
            &format!("std.fs.remove_file([[{}]])", p.display()),
            &host(),
        )
        .expect("remove");
        assert!(!p.exists());
    }

    // ── std.path additions ──────────────────────────────────────────

    #[test]
    fn path_from_joins_segments() {
        let e = engine();
        let r = e
            .eval(
                "local p = std.path.from({'foo', 'bar', 'baz.txt'}); return string.find(p, 'baz.txt') ~= nil and 1 or 0",
                &host(),
            )
            .expect("eval");
        assert_eq!(r.value, 1);
    }

    // ── rhai.runtime ────────────────────────────────────────────────

    #[test]
    fn atomic_write_and_read() {
        let dir = TempDir::new().expect("tempdir");
        let p = dir.path().join("atomic.txt");
        let e = engine();
        e.eval(
            &format!("rhai.runtime.atomic_write([[{}]], 'atomic content')", p.display()),
            &host(),
        )
        .expect("atomic_write");
        let content = std::fs::read_to_string(&p).expect("read");
        assert_eq!(content, "atomic content");
    }

    // ── fs: rename / remove_dir_all / symlink_metadata ──────────────

    #[test]
    fn fs_rename_moves_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, "moved").unwrap();
        let e = engine();
        e.eval(
            &format!("std.fs.rename([[{}]], [[{}]])", src.display(), dst.display()),
            &host(),
        ).expect("rename");
        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "moved");
    }

    #[test]
    fn fs_remove_dir_all_deletes_tree() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), "x").unwrap();
        let e = engine();
        e.eval(
            &format!("std.fs.remove_dir_all([[{}]])", sub.display()),
            &host(),
        ).expect("remove_dir_all");
        assert!(!sub.exists());
    }

    #[test]
    fn fs_symlink_metadata_file() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("sym.txt");
        std::fs::write(&p, "data").unwrap();
        let e = engine();
        let r = e.eval(
            &format!("local m = std.fs.symlink_metadata([[{}]]); return m.is_file and 1 or 0", p.display()),
            &host(),
        ).expect("symlink_metadata");
        assert_eq!(r.value, 1);
    }

    // ── process: id / list ──────────────────────────────────────────

    #[test]
    fn process_id_returns_pid() {
        let e = engine();
        let r = e.eval("return std.process.id()", &host()).expect("id");
        assert!(r.value > 0, "pid must be positive");
    }

    #[test]
    fn process_list_returns_entries() {
        let e = engine();
        let r = e.eval("local lst = std.process.list(); return #lst", &host()).expect("list");
        assert!(r.value > 0, "process list must not be empty");
    }

    // ── time: Duration ──────────────────────────────────────────────

    #[test]
    fn time_duration_from_millis() {
        let e = engine();
        let r = e.eval("return std.time.Duration.from_millis(5000)", &host()).expect("duration");
        assert_eq!(r.value, 5000);
    }

    #[test]
    fn time_duration_from_secs() {
        let e = engine();
        let r = e.eval("return std.time.Duration.from_secs(3)", &host()).expect("duration");
        assert_eq!(r.value, 3000);
    }

    // ── rh::fail ────────────────────────────────────────────────────

    #[test]
    fn rh_fail_triggers_error() {
        let e = engine();
        let r = e.eval(
            "local ok, err = pcall(function() rh.fail('test_failure') end); return ok and 0 or 1",
            &host(),
        );
        if let Ok(r) = r {
            assert_eq!(r.value, 1, "expected rh.fail to trigger error");
        }
    }

    // ── fnv1a64 ─────────────────────────────────────────────────────

    #[test]
    fn fnv1a64_deterministic() {
        let e = engine();
        let r = e.eval("return #rhai.hash.fnv1a64('hello')", &host()).expect("fnv");
        assert_eq!(r.value, 16, "fnv1a64 hex must be 16 chars");
    }

    #[test]
    fn fnv1a64_different_inputs() {
        let e = engine();
        let r = e.eval(
            "local h1 = rhai.hash.fnv1a64('a'); local h2 = rhai.hash.fnv1a64('b'); return h1 ~= h2 and 1 or 0",
            &host(),
        ).expect("fnv compare");
        assert_eq!(r.value, 1);
    }
}
