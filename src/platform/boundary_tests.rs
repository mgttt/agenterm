//! Repository source-boundary regression tests.

use std::fs;
use std::path::{Path, PathBuf};

#[path = "adapters/linux/contract_manifest.rs"]
mod linux_adapter_contract;
#[path = "adapters/macos/contract_manifest.rs"]
mod macos_adapter_contract;
#[path = "adapters/windows/contract_manifest.rs"]
mod windows_adapter_contract;

const NATIVE_BOUNDARY_MARKERS: &[&str] = &[
    "target_os",
    "target_family",
    "windows_sys",
    "std::os::windows",
    "std::os::unix",
    "libc::",
    "objc2::",
    "core_foundation::",
    "rmux_pty::",
    "softbuffer::",
    "winit::",
    "raw_window_handle::",
];

const PRODUCT_COUPLING_MARKERS: &[&str] = &[
    "crate::client",
    "crate::commands",
    "crate::control_center",
    "crate::fleet",
    "crate::instances",
    "crate::theme",
    "crate::ui_",
    "agenterm::",
    "AGENTERM_",
    "agenterm-server",
];

const PLATFORM_CRATE: &str = "crates/agenterm-platform";

const SUBSYSTEM_ENTRYPOINTS: &[&str] = &[
    "src/bin/agenterm.rs",
    "src/bin/agenterm-server.rs",
    "src/bin/agenterm-cc.rs",
];
const WINDOWS_SUBSYSTEM_ATTRIBUTE: &str = "#![cfg_attr(windows, windows_subsystem = \"windows\")]";

#[test]
fn production_sources_use_platform_crate_as_the_only_native_boundary() {
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
        let source = fs::read_to_string(&path).expect("read Rust source");
        // Product-specific frontends and Control Center/Fleet extensions remain
        // ordinary main-crate code: they may call the public platform API, but
        // they do not receive an exemption for target selection or native types.
        let source = if SUBSYSTEM_ENTRYPOINTS.contains(&relative.as_str()) {
            source.replacen(WINDOWS_SUBSYSTEM_ATTRIBUTE, "", 1)
        } else {
            source
        };
        let production = mask_test_items(&mask_comments_and_strings(&source));
        for marker in NATIVE_BOUNDARY_MARKERS {
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
        if let Some((position, target)) = find_cfg_target(&production) {
            let line = production[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            violations.push(format!(
                "{relative}:{line}: forbidden platform cfg target `{target}`"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "production OS boundaries must stay in crates/agenterm-platform/** (apart from the exact subsystem attributes removed above):\n{}",
        violations.join("\n")
    );
}

#[test]
fn platform_crate_native_mechanics_stay_in_selected_and_adapters() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let platform_root = root.join(PLATFORM_CRATE).join("src");
    let mut sources = Vec::new();
    collect_rust_sources(&platform_root, &mut sources);
    sources.sort();

    let mut violations = Vec::new();
    for path in sources {
        let relative = path
            .strip_prefix(&root)
            .expect("source is below manifest root")
            .to_string_lossy()
            .replace('\\', "/");
        if relative == format!("{PLATFORM_CRATE}/src/selected.rs")
            || relative.starts_with(&format!("{PLATFORM_CRATE}/src/adapters/"))
        {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read Rust source");
        let production = mask_test_items(&mask_comments_and_strings(&source));
        for marker in NATIVE_BOUNDARY_MARKERS {
            if let Some(position) = production.find(marker) {
                let line = production[..position]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                violations.push(format!(
                    "{relative}:{line}: native marker `{marker}` must stay in selected.rs or adapters"
                ));
            }
        }
        if let Some((position, target)) = find_cfg_target(&production) {
            let line = production[..position]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            violations.push(format!(
                "{relative}:{line}: platform cfg target `{target}` must stay in selected.rs or adapters"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "platform contracts/services contain native mechanics or OS selection:\n{}",
        violations.join("\n")
    );
}

#[test]
fn platform_crate_has_no_agenterm_product_dependency_or_source_coupling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let platform_root = root.join(PLATFORM_CRATE);
    let manifest = fs::read_to_string(platform_root.join("Cargo.toml"))
        .expect("read agenterm-platform manifest");
    let manifest_without_comments = manifest
        .lines()
        .map(|line| line.split_once('#').map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    let mut violations = Vec::new();

    for (line_number, line) in manifest_without_comments.lines().enumerate() {
        let trimmed = line.trim();
        let compact = trimmed.split_whitespace().collect::<String>();
        if compact.starts_with("agenterm=")
            || compact.starts_with("agenterm.")
            || (compact.starts_with('[') && compact.ends_with(".agenterm]"))
            || compact.contains("package=\"agenterm\"")
            || compact.contains("path=\"../..\"")
        {
            violations.push(format!(
                "{PLATFORM_CRATE}/Cargo.toml:{}: reverse dependency on the Agenterm product crate",
                line_number + 1
            ));
        }
    }

    let mut sources = Vec::new();
    collect_rust_sources(&platform_root.join("src"), &mut sources);
    sources.sort();
    for path in sources {
        let relative = path
            .strip_prefix(&root)
            .expect("source is below manifest root")
            .to_string_lossy()
            .replace('\\', "/");
        let source = fs::read_to_string(&path).expect("read Rust source");
        let production = mask_test_items(&mask_comments(&source));
        for marker in PRODUCT_COUPLING_MARKERS {
            if let Some(position) = production.find(marker) {
                let line = production[..position]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                violations.push(format!(
                    "{relative}:{line}: product coupling marker `{marker}`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "agenterm-platform must be independently consumable and product-neutral:\n{}",
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

fn find_cfg_target(source: &str) -> Option<(usize, &'static str)> {
    let bytes = source.as_bytes();
    for name in ["cfg", "cfg_attr"] {
        let mut cursor = 0;
        while let Some(relative) = source[cursor..].find(name) {
            let start = cursor + relative;
            let before_is_identifier =
                start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
            let after_name = start + name.len();
            let after_is_identifier = after_name < bytes.len()
                && (bytes[after_name].is_ascii_alphanumeric() || bytes[after_name] == b'_');
            if before_is_identifier || after_is_identifier {
                cursor = after_name;
                continue;
            }

            let mut open = after_name;
            while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                open += 1;
            }
            if name == "cfg" && open < bytes.len() && bytes[open] == b'!' {
                open += 1;
                while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                    open += 1;
                }
            }
            if open >= bytes.len() || bytes[open] != b'(' {
                cursor = after_name;
                continue;
            }

            let mut depth = 0_u32;
            let mut end = open;
            for (offset, byte) in bytes[open..].iter().copied().enumerate() {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + offset + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let expression = &source[open + 1..end];
            for (offset, token) in identifier_tokens(expression) {
                if token == "windows" {
                    return Some((open + 1 + offset, "windows"));
                }
                if token == "unix" {
                    return Some((open + 1 + offset, "unix"));
                }
            }
            cursor = end.max(after_name);
        }
    }
    None
}

fn identifier_tokens(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut cursor = 0;
    std::iter::from_fn(move || {
        let bytes = source.as_bytes();
        while cursor < bytes.len()
            && !(bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_')
        {
            cursor += 1;
        }
        if cursor == bytes.len() {
            return None;
        }
        let start = cursor;
        cursor += 1;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
        {
            cursor += 1;
        }
        Some((start, &source[start..cursor]))
    })
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

fn mask_comments(source: &str) -> String {
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
            cursor += 1;
            while cursor < bytes.len() {
                if bytes[cursor] == b'\\' {
                    cursor = (cursor + 2).min(bytes.len());
                } else if bytes[cursor] == b'"' {
                    cursor += 1;
                    break;
                } else {
                    cursor += 1;
                }
            }
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

#[test]
fn cfg_target_scan_detects_nested_and_spaced_target_predicates() {
    let nested = "#[cfg(all(feature = \"native\", windows))] fn native() {}";
    let spaced = "if cfg ! ( any(unix, feature = \"portable\") ) {}";
    assert_eq!(
        find_cfg_target(nested).map(|(_, target)| target),
        Some("windows")
    );
    assert_eq!(
        find_cfg_target(spaced).map(|(_, target)| target),
        Some("unix")
    );
    assert_eq!(find_cfg_target("let windows = Vec::new();"), None);
}

#[test]
fn product_coupling_mask_keeps_literals_but_ignores_tests_and_comments() {
    let fixture = r#"
// AGENTERM_COMMENT is not product code.
fn product_default() { let _ = "AGENTERM_INSTANCE_DIR"; }
#[cfg(test)]
mod tests {
    fn fixture() { crate::commands::run(); }
}
"#;
    let masked = mask_test_items(&mask_comments(fixture));
    assert!(masked.contains("AGENTERM_INSTANCE_DIR"));
    assert!(!masked.contains("AGENTERM_COMMENT"));
    assert!(!masked.contains("crate::commands"));
}

#[test]
fn all_three_adapters_satisfy_the_same_contract() {
    use crate::platform::contract::adapter::validate_adapter_contract;

    let declarations = [
        (
            &windows_adapter_contract::DECLARATION,
            windows_adapter_contract::unsupported_probe(),
            windows_adapter_contract::failed_probe(),
        ),
        (
            &linux_adapter_contract::DECLARATION,
            linux_adapter_contract::unsupported_probe(),
            linux_adapter_contract::failed_probe(),
        ),
        (
            &macos_adapter_contract::DECLARATION,
            macos_adapter_contract::unsupported_probe(),
            macos_adapter_contract::failed_probe(),
        ),
    ];

    for (declaration, unsupported, failed) in declarations {
        validate_adapter_contract(declaration, unsupported, failed)
            .unwrap_or_else(|error| panic!("adapter contract mismatch: {error}"));
    }
}
