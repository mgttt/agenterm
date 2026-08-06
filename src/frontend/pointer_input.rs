//! Parsing for the `ui-input` command family.
//!
//! `ui-input` gives an agent the same affordances a human has: click, drag,
//! wheel, and multi-click gestures addressed in **pixels**, in the same
//! coordinate space `ui-snapshot` already reports element bounds in. That lets
//! a caller read a snapshot, find `tabs[0].actions.close.bounds`, and press its
//! centre — a perceive-then-act loop over the control protocol alone.
//!
//! This module only parses and validates. Turning a request into a real event
//! is the frontend's job, and it must do so by synthesizing the same
//! `PixelWindowEvent` a window manager would deliver, so a synthetic gesture
//! and a human one cannot diverge. Anything that hit-tests a second time is a
//! bug: the composer already shipped once with selection on Unix and none on
//! Windows precisely because two implementations drifted.

/// Which pointer button a request refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerButtonKind {
    Left,
    Right,
    Middle,
}

/// What the pointer should do at the requested position.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PointerActionKind {
    Press,
    Release,
    Move,
}

/// Modifier keys held during the gesture.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct RequestedModifiers {
    pub(crate) control: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) meta: bool,
}

/// A validated `ui-input` request.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PointerRequest {
    Pointer {
        x: f64,
        y: f64,
        button: PointerButtonKind,
        action: PointerActionKind,
        /// 1 for a single click, 2 to promote to word select, 3 to line select.
        count: u8,
        modifiers: RequestedModifiers,
    },
    Wheel {
        x: f64,
        y: f64,
        delta_y: f64,
        /// Wheel notches are line-based; trackpads report pixels.
        line_based: bool,
        modifiers: RequestedModifiers,
    },
    /// A keystroke aimed at whatever surface currently holds focus.
    ///
    /// Distinct from `send-keys`, which addresses a pane and writes to its PTY.
    /// This drives the window's own key handling, so it can reach the composer,
    /// dialogs and shortcuts the way a human keyboard does.
    Key {
        key: KeyRequest,
        modifiers: RequestedModifiers,
    },
}

/// Which key to synthesize.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum KeyRequest {
    /// Literal text, typically one character.
    Text(String),
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
}

/// Largest accepted click count. Triple-click (line select) is the deepest
/// gesture any surface here promotes to, so a larger value is a caller error
/// rather than something to clamp silently.
const MAX_CLICK_COUNT: u8 = 3;

/// Reject absurd coordinates outright. A window this large does not exist, and
/// accepting NaN or infinity would propagate into hit-testing arithmetic.
const MAX_COORDINATE: f64 = 100_000.0;

pub(crate) fn parse_pointer_request(args: &[String]) -> Result<PointerRequest, String> {
    let kind = args
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "ui_input_kind_missing".to_owned())?;
    let options = parse_pairs(&args[2..])?;
    let modifiers = parse_modifiers(options.get("--mods").map(String::as_str))?;
    if kind == "key" {
        let key = options
            .get("--key")
            .ok_or_else(|| "ui_input_key_missing".to_owned())?;
        return Ok(PointerRequest::Key {
            key: parse_key(key)?,
            modifiers,
        });
    }
    // Pointer and wheel are positional; `key` above goes to the focused surface
    // and has no coordinates, so it must not be forced to supply them.
    let x = coordinate(&options, "--x")?;
    let y = coordinate(&options, "--y")?;

    match kind {
        "pointer" => {
            let button = match options.get("--button").map(String::as_str) {
                None | Some("left") => PointerButtonKind::Left,
                Some("right") => PointerButtonKind::Right,
                Some("middle") => PointerButtonKind::Middle,
                Some(_) => return Err("ui_input_button_invalid".to_owned()),
            };
            let action = match options.get("--action").map(String::as_str) {
                None | Some("press") => PointerActionKind::Press,
                Some("release") => PointerActionKind::Release,
                Some("move") => PointerActionKind::Move,
                Some(_) => return Err("ui_input_action_invalid".to_owned()),
            };
            let count = match options.get("--count") {
                None => 1,
                Some(value) => value
                    .parse::<u8>()
                    .ok()
                    .filter(|count| (1..=MAX_CLICK_COUNT).contains(count))
                    .ok_or_else(|| "ui_input_count_invalid".to_owned())?,
            };
            Ok(PointerRequest::Pointer {
                x,
                y,
                button,
                action,
                count,
                modifiers,
            })
        }
        "wheel" => {
            let delta_y = options
                .get("--delta-y")
                .ok_or_else(|| "ui_input_delta_missing".to_owned())?
                .parse::<f64>()
                .ok()
                .filter(|delta| delta.is_finite())
                .ok_or_else(|| "ui_input_delta_invalid".to_owned())?;
            // Callers speak in notches unless they explicitly ask for pixels,
            // matching how `scroll-pane` already talks about rows.
            let line_based = match options.get("--units").map(String::as_str) {
                None | Some("lines") => true,
                Some("pixels") => false,
                Some(_) => return Err("ui_input_units_invalid".to_owned()),
            };
            Ok(PointerRequest::Wheel {
                x,
                y,
                delta_y,
                line_based,
                modifiers,
            })
        }
        _ => Err("ui_input_kind_invalid".to_owned()),
    }
}

fn coordinate(
    options: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<f64, String> {
    options
        .get(key)
        .ok_or_else(|| format!("ui_input_coordinate_missing{key}"))?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && value.abs() <= MAX_COORDINATE)
        .ok_or_else(|| "ui_input_coordinate_invalid".to_owned())
}

/// Resolves a `--key` name to the keystroke to synthesize.
///
/// Named keys are matched case-insensitively; anything else is taken as literal
/// text so a caller can type a character (or a whole word) directly.
fn parse_key(value: &str) -> Result<KeyRequest, String> {
    if value.is_empty() {
        return Err("ui_input_key_invalid".to_owned());
    }
    Ok(match value.to_ascii_lowercase().as_str() {
        "enter" | "return" => KeyRequest::Enter,
        "backspace" => KeyRequest::Backspace,
        "delete" | "del" => KeyRequest::Delete,
        "tab" => KeyRequest::Tab,
        "escape" | "esc" => KeyRequest::Escape,
        "left" | "arrowleft" => KeyRequest::ArrowLeft,
        "right" | "arrowright" => KeyRequest::ArrowRight,
        "up" | "arrowup" => KeyRequest::ArrowUp,
        "down" | "arrowdown" => KeyRequest::ArrowDown,
        "home" => KeyRequest::Home,
        "end" => KeyRequest::End,
        "space" => KeyRequest::Text(" ".to_owned()),
        _ => KeyRequest::Text(value.to_owned()),
    })
}

fn parse_modifiers(value: Option<&str>) -> Result<RequestedModifiers, String> {
    let mut modifiers = RequestedModifiers::default();
    let Some(value) = value else {
        return Ok(modifiers);
    };
    for name in value.split(',').filter(|name| !name.is_empty()) {
        match name {
            "ctrl" | "control" => modifiers.control = true,
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            // `cmd` and `super` name the same physical key on macOS and Linux;
            // accept both so a caller need not know which host it drives.
            "meta" | "cmd" | "super" => modifiers.meta = true,
            _ => return Err("ui_input_modifier_invalid".to_owned()),
        }
    }
    Ok(modifiers)
}

fn parse_pairs(args: &[String]) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut options = std::collections::BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let key = args[index].as_str();
        if !key.starts_with("--") {
            return Err("ui_input_argument_invalid".to_owned());
        }
        let value = args
            .get(index + 1)
            .ok_or_else(|| "ui_input_argument_missing_value".to_owned())?;
        if options.insert(key.to_owned(), value.clone()).is_some() {
            return Err("ui_input_argument_duplicated".to_owned());
        }
        index += 2;
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn pointer_press_defaults_to_a_single_left_click() {
        let request =
            parse_pointer_request(&args(&["ui-input", "pointer", "--x", "120", "--y", "48"]))
                .expect("valid request");
        assert_eq!(
            request,
            PointerRequest::Pointer {
                x: 120.0,
                y: 48.0,
                button: PointerButtonKind::Left,
                action: PointerActionKind::Press,
                count: 1,
                modifiers: RequestedModifiers::default(),
            }
        );
    }

    #[test]
    fn click_count_promotes_up_to_a_triple_click_and_no_further() {
        for (count, expected) in [("1", 1_u8), ("2", 2), ("3", 3)] {
            let request = parse_pointer_request(&args(&[
                "ui-input", "pointer", "--x", "1", "--y", "1", "--count", count,
            ]))
            .expect("valid count");
            let PointerRequest::Pointer { count, .. } = request else {
                panic!("expected a pointer request");
            };
            assert_eq!(count, expected);
        }
        // Four clicks is a caller error, not something to silently clamp to 3.
        assert_eq!(
            parse_pointer_request(&args(&[
                "ui-input", "pointer", "--x", "1", "--y", "1", "--count", "4",
            ])),
            Err("ui_input_count_invalid".to_owned())
        );
        assert_eq!(
            parse_pointer_request(&args(&[
                "ui-input", "pointer", "--x", "1", "--y", "1", "--count", "0",
            ])),
            Err("ui_input_count_invalid".to_owned())
        );
    }

    #[test]
    fn modifiers_accept_platform_aliases_for_the_same_key() {
        for name in ["meta", "cmd", "super"] {
            let parsed = parse_modifiers(Some(name)).expect("valid modifier");
            assert!(parsed.meta, "{name} should map to meta");
        }
        let combined = parse_modifiers(Some("shift,ctrl")).expect("valid modifiers");
        assert!(combined.shift && combined.control);
        assert!(!combined.alt && !combined.meta);
        assert_eq!(
            parse_modifiers(Some("hyper")),
            Err("ui_input_modifier_invalid".to_owned())
        );
    }

    #[test]
    fn non_finite_and_absurd_coordinates_are_rejected() {
        for value in ["NaN", "inf", "-inf", "1e9"] {
            assert_eq!(
                parse_pointer_request(&args(&["ui-input", "pointer", "--x", value, "--y", "1"])),
                Err("ui_input_coordinate_invalid".to_owned()),
                "{value} should not reach hit-testing"
            );
        }
    }

    #[test]
    fn key_requests_need_no_coordinates_and_name_common_keys() {
        // Coordinates are meaningless here: the keystroke goes to whatever
        // surface holds focus, so requiring --x/--y would be noise.
        let request =
            parse_pointer_request(&args(&["ui-input", "key", "--key", "Enter"])).expect("valid");
        assert_eq!(
            request,
            PointerRequest::Key {
                key: KeyRequest::Enter,
                modifiers: RequestedModifiers::default(),
            }
        );
        for (name, expected) in [
            ("escape", KeyRequest::Escape),
            ("ESC", KeyRequest::Escape),
            ("Backspace", KeyRequest::Backspace),
            ("left", KeyRequest::ArrowLeft),
            ("ArrowDown", KeyRequest::ArrowDown),
        ] {
            let parsed = parse_pointer_request(&args(&["ui-input", "key", "--key", name]))
                .expect("valid key");
            let PointerRequest::Key { key, .. } = parsed else {
                panic!("expected a key request");
            };
            assert_eq!(key, expected, "{name}");
        }
    }

    #[test]
    fn unrecognised_key_names_are_typed_as_literal_text() {
        // So a caller can type a character, or a whole word, without escaping.
        for value in ["X", "中", "hello"] {
            let parsed = parse_pointer_request(&args(&["ui-input", "key", "--key", value]))
                .expect("valid text key");
            let PointerRequest::Key { key, .. } = parsed else {
                panic!("expected a key request");
            };
            assert_eq!(key, KeyRequest::Text(value.to_owned()));
        }
        assert_eq!(
            parse_pointer_request(&args(&["ui-input", "key"])),
            Err("ui_input_key_missing".to_owned())
        );
    }

    #[test]
    fn wheel_defaults_to_line_units_and_requires_a_delta() {
        let request = parse_pointer_request(&args(&[
            "ui-input",
            "wheel",
            "--x",
            "10",
            "--y",
            "20",
            "--delta-y",
            "-3",
        ]))
        .expect("valid wheel");
        assert_eq!(
            request,
            PointerRequest::Wheel {
                x: 10.0,
                y: 20.0,
                delta_y: -3.0,
                line_based: true,
                modifiers: RequestedModifiers::default(),
            }
        );
        assert_eq!(
            parse_pointer_request(&args(&["ui-input", "wheel", "--x", "1", "--y", "1"])),
            Err("ui_input_delta_missing".to_owned())
        );
    }

    #[test]
    fn wheel_can_ask_for_pixel_units() {
        let request = parse_pointer_request(&args(&[
            "ui-input",
            "wheel",
            "--x",
            "1",
            "--y",
            "1",
            "--delta-y",
            "40",
            "--units",
            "pixels",
        ]))
        .expect("valid wheel");
        let PointerRequest::Wheel { line_based, .. } = request else {
            panic!("expected a wheel request");
        };
        assert!(!line_based);
    }

    #[test]
    fn malformed_argument_lists_are_rejected_rather_than_half_applied() {
        assert_eq!(
            parse_pointer_request(&args(&["ui-input", "pointer", "--x"])),
            Err("ui_input_argument_missing_value".to_owned())
        );
        assert_eq!(
            parse_pointer_request(&args(&["ui-input", "pointer", "x", "1"])),
            Err("ui_input_argument_invalid".to_owned())
        );
        assert_eq!(
            parse_pointer_request(&args(&[
                "ui-input", "pointer", "--x", "1", "--x", "2", "--y", "1",
            ])),
            Err("ui_input_argument_duplicated".to_owned())
        );
        assert_eq!(
            parse_pointer_request(&args(&["ui-input"])),
            Err("ui_input_kind_missing".to_owned())
        );
        assert_eq!(
            parse_pointer_request(&args(&["ui-input", "drag", "--x", "1", "--y", "1"])),
            Err("ui_input_kind_invalid".to_owned())
        );
    }
}
