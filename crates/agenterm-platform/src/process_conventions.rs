//! Pure host process-parameter conventions without native process access.

pub use crate::contract::process_conventions::{
    InvalidEnvironmentEntryPolicy, WindowsCommandLineError, WindowsEnvironmentBlockError,
};

/// Encode UTF-8 argv using the quoting rules consumed by the Windows CRT and
/// `CommandLineToArgvW`.
///
/// The executable is always quoted. Remaining arguments are quoted only when
/// needed, with backslashes before quotes and closing quotes escaped exactly.
/// This pure helper is available on every host so build and orchestration code
/// can prepare Windows inputs without opening a native process.
pub fn windows_command_line(args: &[String]) -> Result<String, WindowsCommandLineError> {
    if args.is_empty() {
        return Err(WindowsCommandLineError::EmptyCommand);
    }
    if let Some(index) = args.iter().position(|argument| argument.contains('\0')) {
        return Err(WindowsCommandLineError::ArgumentContainsNul { index });
    }

    let mut encoded = String::new();
    push_windows_argument(&mut encoded, &args[0], true);
    for argument in &args[1..] {
        encoded.push(' ');
        push_windows_argument(&mut encoded, argument, false);
    }
    Ok(encoded)
}

/// Encode UTF-8 environment entries as the UTF-16 block accepted by
/// `CreateProcessW`.
///
/// The result always ends with two NUL code units. Valid entries are stably
/// sorted by a locale-independent Unicode uppercase key as required by the
/// Windows environment-block contract; equal folded names retain caller order,
/// so duplicate-name policy remains with the caller.
pub fn windows_environment_block(
    entries: &[(String, String)],
    invalid: InvalidEnvironmentEntryPolicy,
) -> Result<Vec<u16>, WindowsEnvironmentBlockError> {
    let mut valid = Vec::with_capacity(entries.len());
    for (index, (key, value)) in entries.iter().enumerate() {
        if let Some(error) = validate_environment_entry(index, key, value) {
            match invalid {
                InvalidEnvironmentEntryPolicy::Reject => return Err(error),
                InvalidEnvironmentEntryPolicy::Skip => continue,
            }
        }
        valid.push((
            key.to_uppercase().encode_utf16().collect::<Vec<_>>(),
            key,
            value,
        ));
    }
    valid.sort_by(|left, right| left.0.cmp(&right.0));

    let mut encoded = String::new();
    for (_, key, value) in &valid {
        encoded.push_str(key);
        encoded.push('=');
        encoded.push_str(value);
        encoded.push('\0');
    }
    if valid.is_empty() {
        encoded.push('\0');
    }
    encoded.push('\0');
    Ok(encoded.encode_utf16().collect())
}

fn validate_environment_entry(
    index: usize,
    key: &str,
    value: &str,
) -> Option<WindowsEnvironmentBlockError> {
    if key.is_empty() {
        Some(WindowsEnvironmentBlockError::EmptyKey { index })
    } else if key.contains('=') {
        Some(WindowsEnvironmentBlockError::KeyContainsEquals { index })
    } else if key.contains('\0') {
        Some(WindowsEnvironmentBlockError::KeyContainsNul { index })
    } else if value.contains('\0') {
        Some(WindowsEnvironmentBlockError::ValueContainsNul { index })
    } else {
        None
    }
}

fn push_windows_argument(output: &mut String, argument: &str, force_quote: bool) {
    let quote = force_quote
        || argument.is_empty()
        || argument
            .chars()
            .any(|character| matches!(character, ' ' | '\t' | '"'));
    if !quote {
        output.push_str(argument);
        return;
    }

    output.push('"');
    let mut backslashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        for _ in 0..backslashes {
            output.push('\\');
        }
        if character == '"' {
            for _ in 0..backslashes {
                output.push('\\');
            }
            output.push('\\');
        }
        output.push(character);
        backslashes = 0;
    }
    for _ in 0..backslashes {
        output.push('\\');
        output.push('\\');
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment_text(entries: &[(String, String)]) -> String {
        windows_environment_block(entries, InvalidEnvironmentEntryPolicy::Skip)
            .expect("skip policy cannot reject an entry")
            .into_iter()
            .map(|unit| {
                if unit == 0 {
                    "<NUL>".to_owned()
                } else {
                    char::from_u32(u32::from(unit))
                        .unwrap_or('\u{fffd}')
                        .to_string()
                }
            })
            .collect()
    }

    #[test]
    fn windows_command_line_preserves_crt_argument_boundaries() {
        let args = vec![
            "cmd.exe".to_owned(),
            "plain".to_owned(),
            String::new(),
            "two words".to_owned(),
            r"C:\path with space\".to_owned(),
            r#"x\"y"#.to_owned(),
            r#"say "hello""#.to_owned(),
            "tab\tvalue".to_owned(),
        ];
        assert_eq!(
            windows_command_line(&args).unwrap(),
            "\"cmd.exe\" plain \"\" \"two words\" \"C:\\path with space\\\\\" \"x\\\\\\\"y\" \"say \\\"hello\\\"\" \"tab\tvalue\""
        );
        assert_eq!(
            windows_command_line(&[]).unwrap_err(),
            WindowsCommandLineError::EmptyCommand
        );
        assert_eq!(
            windows_command_line(&["cmd.exe".to_owned(), "bad\0arg".to_owned()]).unwrap_err(),
            WindowsCommandLineError::ArgumentContainsNul { index: 1 }
        );
    }

    #[test]
    fn windows_environment_block_preserves_policy_and_terminators() {
        let entries = vec![
            (String::new(), "drop".to_owned()),
            ("GOOD".to_owned(), "keep".to_owned()),
            ("K=E".to_owned(), "drop".to_owned()),
            ("KEY_NUL\0".to_owned(), "drop".to_owned()),
            ("VALUE_NUL".to_owned(), "drop\0".to_owned()),
            ("ALSO_GOOD".to_owned(), "v=1".to_owned()),
        ];
        assert_eq!(
            environment_text(&entries),
            "ALSO_GOOD=v=1<NUL>GOOD=keep<NUL><NUL>"
        );
        assert_eq!(environment_text(&[]), "<NUL><NUL>");
        for (entry, expected) in [
            (
                (String::new(), "value".to_owned()),
                WindowsEnvironmentBlockError::EmptyKey { index: 0 },
            ),
            (
                ("K=E".to_owned(), "value".to_owned()),
                WindowsEnvironmentBlockError::KeyContainsEquals { index: 0 },
            ),
            (
                ("K\0E".to_owned(), "value".to_owned()),
                WindowsEnvironmentBlockError::KeyContainsNul { index: 0 },
            ),
            (
                ("KEY".to_owned(), "v\0x".to_owned()),
                WindowsEnvironmentBlockError::ValueContainsNul { index: 0 },
            ),
        ] {
            assert_eq!(
                windows_environment_block(&[entry], InvalidEnvironmentEntryPolicy::Reject)
                    .unwrap_err(),
                expected
            );
        }
        assert_eq!(
            environment_text(&[("NAME".to_owned(), "中文=ok".to_owned())]),
            "NAME=中文=ok<NUL><NUL>"
        );
        assert_eq!(
            environment_text(&[
                ("zeta".to_owned(), "last".to_owned()),
                ("Path".to_owned(), "first-duplicate".to_owned()),
                ("alpha".to_owned(), "first".to_owned()),
                ("PATH".to_owned(), "second-duplicate".to_owned()),
                ("中文".to_owned(), "unicode".to_owned()),
            ]),
            "alpha=first<NUL>Path=first-duplicate<NUL>PATH=second-duplicate<NUL>zeta=last<NUL>中文=unicode<NUL><NUL>"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_command_line_round_trips_through_native_parser() {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn LocalFree(memory: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
        }
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn CommandLineToArgvW(command_line: *const u16, argc: *mut i32) -> *mut *mut u16;
        }

        let args = vec![
            "cmd.exe".to_owned(),
            "plain".to_owned(),
            String::new(),
            "two words".to_owned(),
            r"C:\path with space\".to_owned(),
            r#"x\"y"#.to_owned(),
            r#"say "hello""#.to_owned(),
            "制表\t符".to_owned(),
        ];
        let wide = windows_command_line(&args)
            .unwrap()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut argc = 0;
        let argv = unsafe { CommandLineToArgvW(wide.as_ptr(), &raw mut argc) };
        assert!(!argv.is_null());
        let parsed = (0..argc as usize)
            .map(|index| unsafe {
                let value = *argv.add(index);
                let mut len = 0;
                while *value.add(len) != 0 {
                    len += 1;
                }
                String::from_utf16(std::slice::from_raw_parts(value, len)).unwrap()
            })
            .collect::<Vec<_>>();
        unsafe { LocalFree(argv.cast()) };
        assert_eq!(parsed, args);
    }
}
