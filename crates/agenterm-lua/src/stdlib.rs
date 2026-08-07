//! Standard library for Lua scripts: `std.fs`, `std.path`, `std.env`, `std.time`, etc.
//! Aligned with rh's shipped_surfaces API surface.

use std::io::Read;
use std::path::Path;

use mlua::{Lua, Table, Value};

/// Inject the full `std` global table into the Lua runtime.
pub fn inject(lua: &Lua) -> Result<(), mlua::Error> {
    let std_table = lua.create_table()?;
    std_table.set("fs", build_fs(lua)?)?;
    std_table.set("process", build_process(lua)?)?;
    lua.globals().set("std", std_table)?;
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

    Ok(process)
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
}
