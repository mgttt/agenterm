//! Standard library for Lua scripts: `std.fs`, `std.path`, `std.env`, `std.time`, etc.
//! Aligned with rh's shipped_surfaces API surface.

use std::path::Path;

use mlua::{Lua, Table};

/// Inject the full `std` global table into the Lua runtime.
pub fn inject(lua: &Lua) -> Result<(), mlua::Error> {
    let std_table = lua.create_table()?;
    std_table.set("fs", build_fs(lua)?)?;
    lua.globals().set("std", std_table)?;
    Ok(())
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

    // std.fs.metadata(path) → {is_file, is_dir, is_symlink, len, modified} | nil, err
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
        // Should error out (pcall catches it, returning 1)
        if let Ok(r) = r {
            assert_eq!(r.value, 1, "expected error for missing file");
        }
        // direct runtime error also acceptable
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
}
