//! Repository source-boundary regression tests.

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_MARKERS: &[&str] = &[
    "#[cfg(windows",
    "#[cfg(unix",
    "#[cfg_attr(windows",
    "#[cfg_attr(unix",
    "cfg!(windows",
    "cfg!(unix",
    "target_os",
    "target_family",
    "windows_sys",
    "std::os::windows",
    "std::os::unix",
    "libc::",
    "objc2::",
    "core_foundation::",
];

const SUBSYSTEM_ENTRYPOINTS: &[&str] = &[
    "src/bin/agenterm.rs",
    "src/bin/agenterm-server.rs",
    "src/bin/agenterm-cc.rs",
];
const WINDOWS_SUBSYSTEM_ATTRIBUTE: &str = "#![cfg_attr(windows, windows_subsystem = \"windows\")]";

#[test]
fn production_sources_use_platform_as_the_only_native_boundary() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    collect_rust_sources(&root.join("src"), &mut sources);
    sources.sort();

    let mut violations = Vec::new();
    for path in sources {
        let relative = path
            .strip_prefix(&root)
            .expect("source is below manifest root")
            .to_string_lossy()
            .replace('\\', "/");
        if relative.starts_with("src/platform/") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read Rust source");
        let source = if SUBSYSTEM_ENTRYPOINTS.contains(&relative.as_str()) {
            source.replacen(WINDOWS_SUBSYSTEM_ATTRIBUTE, "", 1)
        } else {
            source
        };
        let production = mask_test_items(&mask_comments_and_strings(&source));
        for marker in FORBIDDEN_MARKERS {
            if let Some(position) = production.find(marker) {
                let line = production[..position]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                violations.push(format!(
                    "{relative}:{line}: forbidden platform marker `{marker}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "production OS boundaries must stay in src/platform/**:\n{}",
        violations.join("\n")
    );
}

fn collect_rust_sources(directory: &Path, output: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("read source entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

fn mask_test_items(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let marker = b"#[cfg(test)]";
    let mut cursor = 0;
    while let Some(relative) = bytes[cursor..]
        .windows(marker.len())
        .position(|window| window == marker)
    {
        let start = cursor + relative;
        let Some(open) = bytes[start + marker.len()..]
            .iter()
            .position(|byte| *byte == b'{')
            .map(|offset| start + marker.len() + offset)
        else {
            break;
        };
        let mut depth = 0_u32;
        let mut end = bytes.len();
        for (offset, byte) in bytes[open..].iter().copied().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        for byte in &mut bytes[start..end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        cursor = end;
    }
    String::from_utf8(bytes).expect("mask preserves UTF-8")
}

fn mask_comments_and_strings(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor..].starts_with(b"//") {
            let end = bytes[cursor..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| cursor + offset);
            mask_non_newlines(&mut bytes[cursor..end]);
            cursor = end;
        } else if bytes[cursor..].starts_with(b"/*") {
            let end = bytes[cursor + 2..]
                .windows(2)
                .position(|window| window == b"*/")
                .map_or(bytes.len(), |offset| cursor + 2 + offset + 2);
            mask_non_newlines(&mut bytes[cursor..end]);
            cursor = end;
        } else if bytes[cursor] == b'"' {
            let mut end = cursor + 1;
            while end < bytes.len() {
                if bytes[end] == b'\\' {
                    end = (end + 2).min(bytes.len());
                } else if bytes[end] == b'"' {
                    end += 1;
                    break;
                } else {
                    end += 1;
                }
            }
            mask_non_newlines(&mut bytes[cursor..end]);
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    String::from_utf8(bytes).expect("mask preserves UTF-8")
}

fn mask_non_newlines(bytes: &mut [u8]) {
    for byte in bytes {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

#[test]
fn boundary_mask_ignores_tests_and_comments_but_not_product_code() {
    let fixture = r#"
// windows_sys in a comment is not product code.
fn product() { windows_sys::native_call(); }
#[cfg(test)]
mod tests {
    #[cfg(windows)]
    fn native_fixture() { std::os::windows::ffi::OsStrExt::encode_wide; }
}
"#;
    let masked = mask_test_items(&mask_comments_and_strings(fixture));
    assert!(masked.contains("windows_sys::native_call"));
    assert!(!masked.contains("std::os::windows"));
    assert!(!masked.contains("windows_sys in a comment"));
}
