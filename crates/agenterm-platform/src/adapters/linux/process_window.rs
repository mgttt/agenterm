use crate::contract::process_window::*;

const fn error(
    code: &'static str,
    message: &'static str,
    cause: &'static str,
) -> ProcessWindowError {
    ProcessWindowError::new(code, message, Some(cause))
}

const fn unsupported(message: &'static str) -> ProcessWindowError {
    error("process_window_unsupported", message, "unsupported")
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::*;
    use std::{collections::HashSet, env};
    use x11rb::{
        CURRENT_TIME, NONE,
        connection::Connection,
        protocol::xproto::{
            Atom, AtomEnum, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ButtonPressEvent,
            ButtonReleaseEvent, ConfigureWindowAux, ConnectionExt as _, EventMask,
            GetGeometryReply, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, KeyButMask, KeyPressEvent,
            KeyReleaseEvent, MOTION_NOTIFY_EVENT, MapState, Motion, MotionNotifyEvent, Window,
        },
        rust_connection::RustConnection,
    };

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum SessionKind {
        X11,
        Wayland,
        Unavailable,
    }

    fn classify_session(
        session_type: Option<&str>,
        wayland_display: Option<&str>,
        x11_display: Option<&str>,
    ) -> SessionKind {
        if session_type == Some("wayland") || wayland_display.is_some_and(|value| !value.is_empty())
        {
            SessionKind::Wayland
        } else if session_type == Some("x11") || x11_display.is_some_and(|value| !value.is_empty())
        {
            SessionKind::X11
        } else {
            SessionKind::Unavailable
        }
    }

    fn session_kind() -> SessionKind {
        let session_type = env::var("XDG_SESSION_TYPE").ok();
        let wayland_display = env::var("WAYLAND_DISPLAY").ok();
        let x11_display = env::var("DISPLAY").ok();
        classify_session(
            session_type.as_deref(),
            wayland_display.as_deref(),
            x11_display.as_deref(),
        )
    }

    struct Atoms {
        client_list: Atom,
        client_list_stacking: Atom,
        wm_pid: Atom,
        wm_name: Atom,
        utf8_string: Atom,
        active_window: Atom,
    }

    struct Context {
        connection: RustConnection,
        root: Window,
        atoms: Atoms,
    }

    fn failed(message: &'static str) -> ProcessWindowError {
        error("process_window_failed", message, "platform_error")
    }

    fn atom(connection: &RustConnection, name: &[u8]) -> Result<Atom, ProcessWindowError> {
        connection
            .intern_atom(false, name)
            .map_err(|_| failed("an X11 atom request could not be sent"))?
            .reply()
            .map(|reply| reply.atom)
            .map_err(|_| failed("an X11 atom request failed"))
    }

    fn connect() -> Result<Context, ProcessWindowError> {
        match session_kind() {
            SessionKind::X11 => {}
            SessionKind::Wayland => {
                return Err(unsupported(
                    "Wayland does not permit client-selected process-window observation or native input",
                ));
            }
            SessionKind::Unavailable => {
                return Err(unsupported(
                    "process-window automation requires an X11 display",
                ));
            }
        }
        let (connection, screen) = x11rb::connect(None)
            .map_err(|_| failed("the configured X11 display could not be opened"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| failed("the configured X11 screen does not exist"))?
            .root;
        let atoms = Atoms {
            client_list: atom(&connection, b"_NET_CLIENT_LIST")?,
            client_list_stacking: atom(&connection, b"_NET_CLIENT_LIST_STACKING")?,
            wm_pid: atom(&connection, b"_NET_WM_PID")?,
            wm_name: atom(&connection, b"_NET_WM_NAME")?,
            utf8_string: atom(&connection, b"UTF8_STRING")?,
            active_window: atom(&connection, b"_NET_ACTIVE_WINDOW")?,
        };
        Ok(Context {
            connection,
            root,
            atoms,
        })
    }

    fn windows_property(
        context: &Context,
        property: Atom,
    ) -> Result<Vec<Window>, ProcessWindowError> {
        let reply = context
            .connection
            .get_property(false, context.root, property, AtomEnum::WINDOW, 0, u32::MAX)
            .map_err(|_| failed("the X11 client-list request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 client-list request failed"))?;
        if reply.format != 32 || reply.type_ != u32::from(AtomEnum::WINDOW) {
            return Ok(Vec::new());
        }
        Ok(reply.value32().into_iter().flatten().collect())
    }

    fn client_windows(context: &Context) -> Result<Vec<Window>, ProcessWindowError> {
        let stacking = windows_property(context, context.atoms.client_list_stacking)?;
        let candidates = if stacking.is_empty() {
            windows_property(context, context.atoms.client_list)?
        } else {
            stacking
        };
        let mut seen = HashSet::new();
        Ok(candidates
            .into_iter()
            .filter(|window| seen.insert(*window))
            .collect())
    }

    fn process_id(context: &Context, window: Window) -> Result<Option<u32>, ProcessWindowError> {
        let reply = context
            .connection
            .get_property(
                false,
                window,
                context.atoms.wm_pid,
                AtomEnum::CARDINAL,
                0,
                1,
            )
            .map_err(|_| failed("the X11 process-owner request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 process-owner request failed"))?;
        if reply.format != 32 || reply.type_ != u32::from(AtomEnum::CARDINAL) {
            return Ok(None);
        }
        Ok(reply.value32().and_then(|mut values| values.next()))
    }

    fn is_viewable(context: &Context, window: Window) -> Result<bool, ProcessWindowError> {
        context
            .connection
            .get_window_attributes(window)
            .map_err(|_| failed("the X11 window-state request could not be sent"))?
            .reply()
            .map(|reply| reply.map_state == MapState::VIEWABLE)
            .map_err(|_| failed("the X11 window-state request failed"))
    }

    fn matching_windows(
        context: &Context,
        requested_process_id: u32,
    ) -> Result<Vec<Window>, ProcessWindowError> {
        let mut matches = Vec::new();
        for window in client_windows(context)? {
            if process_id(context, window)? == Some(requested_process_id)
                && is_viewable(context, window)?
            {
                matches.push(window);
            }
        }
        Ok(matches)
    }

    fn select_window(
        matches: &[Window],
        not_found_message: &'static str,
    ) -> Result<Window, ProcessWindowError> {
        match matches {
            [] => Err(error(
                "process_window_not_found",
                not_found_message,
                "not_found",
            )),
            [window] => Ok(*window),
            _ => Err(error(
                "process_window_ambiguous",
                "the process owns multiple viewable X11 client windows",
                "ambiguous",
            )),
        }
    }

    fn required_window(context: &Context, process_id: u32) -> Result<Window, ProcessWindowError> {
        let matches = matching_windows(context, process_id)?;
        select_window(
            &matches,
            "child has no viewable X11 client window owned by the requested process",
        )
    }

    fn title(context: &Context, window: Window) -> Result<String, ProcessWindowError> {
        let modern = context
            .connection
            .get_property(
                false,
                window,
                context.atoms.wm_name,
                context.atoms.utf8_string,
                0,
                16_384,
            )
            .map_err(|_| failed("the X11 window-title request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 window-title request failed"))?;
        if modern.format == 8 && modern.type_ == context.atoms.utf8_string {
            let title = String::from_utf8_lossy(&modern.value).into_owned();
            if !title.is_empty() {
                return Ok(title);
            }
        }
        let legacy = context
            .connection
            .get_property(
                false,
                window,
                AtomEnum::WM_NAME,
                AtomEnum::STRING,
                0,
                16_384,
            )
            .map_err(|_| failed("the legacy X11 window-title request could not be sent"))?
            .reply()
            .map_err(|_| failed("the legacy X11 window-title request failed"))?;
        Ok(if legacy.format == 8 {
            String::from_utf8_lossy(&legacy.value).into_owned()
        } else {
            String::new()
        })
    }

    fn active_window(context: &Context) -> Result<Window, ProcessWindowError> {
        let reply = context
            .connection
            .get_property(
                false,
                context.root,
                context.atoms.active_window,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .map_err(|_| failed("the X11 active-window request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 active-window request failed"))?;
        Ok(reply
            .value32()
            .and_then(|mut values| values.next())
            .unwrap_or(NONE))
    }

    fn geometry(
        context: &Context,
        window: Window,
    ) -> Result<(GetGeometryReply, i16, i16), ProcessWindowError> {
        let geometry = context
            .connection
            .get_geometry(window)
            .map_err(|_| failed("the X11 geometry request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 geometry request failed"))?;
        let translated = context
            .connection
            .translate_coordinates(window, context.root, 0, 0)
            .map_err(|_| failed("the X11 coordinate request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 coordinate request failed"))?;
        if !translated.same_screen {
            return Err(failed("the X11 window is not on the selected root screen"));
        }
        Ok((geometry, translated.dst_x, translated.dst_y))
    }

    fn point_in_window(x: i32, y: i32, width: u16, height: u16) -> bool {
        x >= 0 && y >= 0 && x < i32::from(width) && y < i32::from(height)
    }

    fn event_coordinates(
        x: i32,
        y: i32,
        root_x: i16,
        root_y: i16,
        width: u16,
        height: u16,
    ) -> Result<(i16, i16, i16, i16), ProcessWindowError> {
        if !point_in_window(x, y, width, height) {
            return Err(error(
                "process_window_input_bounds",
                "pointer coordinates are outside the requested process window",
                "invalid_input",
            ));
        }
        let event_x = i16::try_from(x).map_err(|_| {
            error(
                "process_window_input_bounds",
                "pointer coordinates exceed the X11 event range",
                "invalid_input",
            )
        })?;
        let event_y = i16::try_from(y).map_err(|_| {
            error(
                "process_window_input_bounds",
                "pointer coordinates exceed the X11 event range",
                "invalid_input",
            )
        })?;
        let absolute_x = i32::from(root_x)
            .checked_add(x)
            .and_then(|v| i16::try_from(v).ok());
        let absolute_y = i32::from(root_y)
            .checked_add(y)
            .and_then(|v| i16::try_from(v).ok());
        match (absolute_x, absolute_y) {
            (Some(absolute_x), Some(absolute_y)) => Ok((event_x, event_y, absolute_x, absolute_y)),
            _ => Err(error(
                "process_window_input_bounds",
                "pointer coordinates exceed the X11 root-coordinate range",
                "invalid_input",
            )),
        }
    }

    const fn keysym(key: ProcessWindowKey) -> u32 {
        match key {
            ProcessWindowKey::Backspace => 0xff08,
            ProcessWindowKey::Tab => 0xff09,
            ProcessWindowKey::Enter => 0xff0d,
            ProcessWindowKey::Escape => 0xff1b,
            ProcessWindowKey::Home => 0xff50,
            ProcessWindowKey::Left => 0xff51,
            ProcessWindowKey::Up => 0xff52,
            ProcessWindowKey::Right => 0xff53,
            ProcessWindowKey::Down => 0xff54,
            ProcessWindowKey::End => 0xff57,
            ProcessWindowKey::F2 => 0xffbf,
            ProcessWindowKey::Delete => 0xffff,
        }
    }

    fn keycode(context: &Context, requested: u32) -> Result<u8, ProcessWindowError> {
        let setup = context.connection.setup();
        let first = setup.min_keycode;
        let count = setup
            .max_keycode
            .checked_sub(first)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| failed("the X11 keyboard mapping is invalid"))?;
        let reply = context
            .connection
            .get_keyboard_mapping(first, count)
            .map_err(|_| failed("the X11 keyboard-map request could not be sent"))?
            .reply()
            .map_err(|_| failed("the X11 keyboard-map request failed"))?;
        let per_keycode = usize::from(reply.keysyms_per_keycode);
        if per_keycode == 0 {
            return Err(failed("the X11 keyboard mapping is empty"));
        }
        reply
            .keysyms
            .chunks(per_keycode)
            .position(|symbols| symbols.contains(&requested))
            .and_then(|offset| u8::try_from(offset).ok())
            .and_then(|offset| first.checked_add(offset))
            .ok_or_else(|| {
                error(
                    "process_window_input",
                    "the requested key has no X11 keycode",
                    "platform_error",
                )
            })
    }

    fn send<E: Into<[u8; 32]>>(
        context: &Context,
        window: Window,
        mask: EventMask,
        event: E,
    ) -> Result<(), ProcessWindowError> {
        context
            .connection
            .send_event(false, window, mask, event)
            .map_err(|_| failed("the X11 input request could not be sent"))?
            .check()
            .map_err(|_| failed("the X11 server rejected native window input"))?;
        context
            .connection
            .flush()
            .map_err(|_| failed("the X11 input request could not be flushed"))
    }

    pub(super) fn facts(process_id: u32) -> ProcessWindowFacts {
        let Ok(context) = connect() else {
            return unsupported_facts();
        };
        let Ok(matches) = matching_windows(&context, process_id) else {
            return supported_absent_facts();
        };
        let Ok(window) = select_window(&matches, "child has no viewable X11 client window") else {
            return supported_absent_facts();
        };
        let title = title(&context, window).unwrap_or_default();
        let foreground = active_window(&context).unwrap_or(NONE);
        ProcessWindowFacts {
            supported: true,
            present: true,
            window_id: i64::from(window),
            title,
            foreground_window_id: i64::from(foreground),
            is_foreground: window == foreground,
        }
    }

    fn unsupported_facts() -> ProcessWindowFacts {
        ProcessWindowFacts {
            supported: false,
            present: false,
            window_id: 0,
            title: String::new(),
            foreground_window_id: 0,
            is_foreground: false,
        }
    }

    fn supported_absent_facts() -> ProcessWindowFacts {
        ProcessWindowFacts {
            supported: true,
            ..unsupported_facts()
        }
    }

    pub(super) fn key(
        process_id: u32,
        requested_key: ProcessWindowKey,
    ) -> Result<(), ProcessWindowError> {
        let context = connect()?;
        let window = required_window(&context, process_id)?;
        let keycode = keycode(&context, keysym(requested_key))?;
        let base = KeyPressEvent {
            response_type: KEY_PRESS_EVENT,
            detail: keycode,
            sequence: 0,
            time: CURRENT_TIME,
            root: context.root,
            event: window,
            child: NONE,
            root_x: 0,
            root_y: 0,
            event_x: 0,
            event_y: 0,
            state: KeyButMask::default(),
            same_screen: true,
        };
        send(&context, window, EventMask::KEY_PRESS, base)?;
        send(
            &context,
            window,
            EventMask::KEY_RELEASE,
            KeyReleaseEvent {
                response_type: KEY_RELEASE_EVENT,
                ..base
            },
        )
    }

    pub(super) fn pointer(
        process_id: u32,
        action: ProcessWindowPointerAction,
        x: i32,
        y: i32,
    ) -> Result<(), ProcessWindowError> {
        if action == ProcessWindowPointerAction::CaptureChanged {
            return Err(unsupported(
                "X11 has no safe process-local equivalent of Win32 capture-change delivery",
            ));
        }
        let context = connect()?;
        let window = required_window(&context, process_id)?;
        let (geometry, root_x, root_y) = geometry(&context, window)?;
        let (event_x, event_y, absolute_x, absolute_y) =
            event_coordinates(x, y, root_x, root_y, geometry.width, geometry.height)?;
        let button = |response_type, state| ButtonPressEvent {
            response_type,
            detail: 1,
            sequence: 0,
            time: CURRENT_TIME,
            root: context.root,
            event: window,
            child: NONE,
            root_x: absolute_x,
            root_y: absolute_y,
            event_x,
            event_y,
            state,
            same_screen: true,
        };
        let motion = |state| MotionNotifyEvent {
            response_type: MOTION_NOTIFY_EVENT,
            detail: Motion::NORMAL,
            sequence: 0,
            time: CURRENT_TIME,
            root: context.root,
            event: window,
            child: NONE,
            root_x: absolute_x,
            root_y: absolute_y,
            event_x,
            event_y,
            state,
            same_screen: true,
        };
        match action {
            ProcessWindowPointerAction::Click => {
                send(
                    &context,
                    window,
                    EventMask::BUTTON_PRESS,
                    button(BUTTON_PRESS_EVENT, KeyButMask::default()),
                )?;
                send(
                    &context,
                    window,
                    EventMask::BUTTON_RELEASE,
                    ButtonReleaseEvent {
                        response_type: BUTTON_RELEASE_EVENT,
                        ..button(BUTTON_PRESS_EVENT, KeyButMask::BUTTON1)
                    },
                )
            }
            ProcessWindowPointerAction::Down => send(
                &context,
                window,
                EventMask::BUTTON_PRESS,
                button(BUTTON_PRESS_EVENT, KeyButMask::default()),
            ),
            ProcessWindowPointerAction::Move => send(
                &context,
                window,
                EventMask::POINTER_MOTION,
                motion(KeyButMask::default()),
            ),
            ProcessWindowPointerAction::MoveHeld => send(
                &context,
                window,
                EventMask::BUTTON1_MOTION,
                motion(KeyButMask::BUTTON1),
            ),
            ProcessWindowPointerAction::Up => send(
                &context,
                window,
                EventMask::BUTTON_RELEASE,
                ButtonReleaseEvent {
                    response_type: BUTTON_RELEASE_EVENT,
                    ..button(BUTTON_PRESS_EVENT, KeyButMask::BUTTON1)
                },
            ),
            ProcessWindowPointerAction::CaptureChanged => unreachable!(),
        }
    }

    pub(super) fn rect(process_id: u32) -> Result<ProcessWindowRect, ProcessWindowError> {
        let context = connect()?;
        let window = required_window(&context, process_id)?;
        let (geometry, left, top) = geometry(&context, window)?;
        Ok(ProcessWindowRect {
            left: i64::from(left),
            top: i64::from(top),
            right: i64::from(left) + i64::from(geometry.width),
            bottom: i64::from(top) + i64::from(geometry.height),
        })
    }

    pub(super) fn pointer_coordinate_scale(process_id: u32) -> Result<f64, ProcessWindowError> {
        let context = connect()?;
        required_window(&context, process_id)?;
        Ok(1.0)
    }

    pub(super) fn resize(
        process_id: u32,
        width: i32,
        height: i32,
    ) -> Result<(), ProcessWindowError> {
        let width = u32::try_from(width)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                error(
                    "process_window_resize",
                    "X11 window width and height must be positive",
                    "invalid_input",
                )
            })?;
        let height = u32::try_from(height)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                error(
                    "process_window_resize",
                    "X11 window width and height must be positive",
                    "invalid_input",
                )
            })?;
        let context = connect()?;
        let window = required_window(&context, process_id)?;
        context
            .connection
            .configure_window(
                window,
                &ConfigureWindowAux::new().width(width).height(height),
            )
            .map_err(|_| failed("the X11 resize request could not be sent"))?
            .check()
            .map_err(|_| failed("the X11 server rejected the window resize"))?;
        context
            .connection
            .flush()
            .map_err(|_| failed("the X11 resize request could not be flushed"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn selection_fails_closed_for_zero_one_and_multiple_clients() {
            assert_eq!(
                select_window(&[], "missing").unwrap_err().code,
                "process_window_not_found"
            );
            assert_eq!(select_window(&[42], "missing"), Ok(42));
            let ambiguous = select_window(&[42, 43], "missing").unwrap_err();
            assert_eq!(ambiguous.code, "process_window_ambiguous");
            assert_eq!(ambiguous.cause, Some("ambiguous"));
        }

        #[test]
        fn pointer_coordinates_are_strictly_bounded_and_x11_representable() {
            assert!(event_coordinates(0, 0, 10, 20, 80, 24).is_ok());
            assert!(event_coordinates(79, 23, 10, 20, 80, 24).is_ok());
            assert!(event_coordinates(-1, 0, 10, 20, 80, 24).is_err());
            assert!(event_coordinates(80, 23, 10, 20, 80, 24).is_err());
            assert!(event_coordinates(79, 24, 10, 20, 80, 24).is_err());
            assert!(event_coordinates(20_000, 0, 20_000, 0, u16::MAX, 24).is_err());
        }

        #[test]
        fn wayland_takes_precedence_over_xwayland_display() {
            assert_eq!(
                classify_session(Some("wayland"), Some("wayland-0"), Some(":0")),
                SessionKind::Wayland
            );
            assert_eq!(
                classify_session(Some("x11"), None, Some(":0")),
                SessionKind::X11
            );
        }

        #[test]
        fn key_contract_maps_to_stable_x11_keysyms() {
            assert_eq!(keysym(ProcessWindowKey::Tab), 0xff09);
            assert_eq!(keysym(ProcessWindowKey::F2), 0xffbf);
            assert_eq!(keysym(ProcessWindowKey::Delete), 0xffff);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn unsupported_facts() -> ProcessWindowFacts {
    ProcessWindowFacts {
        supported: false,
        present: false,
        window_id: 0,
        title: String::new(),
        foreground_window_id: 0,
        is_foreground: false,
    }
}

pub(crate) fn facts(process_id: u32) -> ProcessWindowFacts {
    #[cfg(target_os = "linux")]
    {
        x11::facts(process_id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = process_id;
        unsupported_facts()
    }
}

pub(crate) fn key(process_id: u32, key: ProcessWindowKey) -> Result<(), ProcessWindowError> {
    #[cfg(target_os = "linux")]
    {
        x11::key(process_id, key)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (process_id, key);
        Err(unsupported(
            "native child-window input is not implemented on this platform",
        ))
    }
}

pub(crate) fn pointer(
    process_id: u32,
    action: ProcessWindowPointerAction,
    x: i32,
    y: i32,
) -> Result<(), ProcessWindowError> {
    #[cfg(target_os = "linux")]
    {
        x11::pointer(process_id, action, x, y)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (process_id, action, x, y);
        Err(unsupported(
            "native child-window input is not implemented on this platform",
        ))
    }
}

pub(crate) fn pointer_coordinate_scale(process_id: u32) -> Result<f64, ProcessWindowError> {
    x11::pointer_coordinate_scale(process_id)
}

pub(crate) fn message(_: u32, _: ProcessWindowMessage) -> Result<isize, ProcessWindowError> {
    Err(unsupported(
        "native child-window messaging has no portable X11 or Wayland equivalent",
    ))
}

pub(crate) fn rect(process_id: u32, _: bool) -> Result<ProcessWindowRect, ProcessWindowError> {
    #[cfg(target_os = "linux")]
    {
        x11::rect(process_id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = process_id;
        Err(unsupported(
            "native child-window bounds are not implemented on this platform",
        ))
    }
}

pub(crate) fn resize(process_id: u32, width: i32, height: i32) -> Result<(), ProcessWindowError> {
    #[cfg(target_os = "linux")]
    {
        x11::resize(process_id, width, height)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (process_id, width, height);
        Err(unsupported(
            "native child-window resize is not implemented on this platform",
        ))
    }
}

pub(crate) fn control_exists(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "Win32-style child-control discovery is unsupported on X11 and Wayland",
    ))
}
pub(crate) fn control_visible(_: u32, _: i32) -> Result<bool, ProcessWindowError> {
    Err(unsupported(
        "Win32-style child-control visibility is unsupported on X11 and Wayland",
    ))
}
pub(crate) fn control_text(_: u32, _: i32) -> Result<String, ProcessWindowError> {
    Err(unsupported(
        "Win32-style child-control text is unsupported on X11 and Wayland",
    ))
}
pub(crate) fn control_set_text(_: u32, _: i32, _: &str) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "Win32-style child-control text mutation is unsupported on X11 and Wayland",
    ))
}
pub(crate) fn control_click(_: u32, _: i32) -> Result<(), ProcessWindowError> {
    Err(unsupported(
        "Win32-style child-control clicks are unsupported on X11 and Wayland",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_specific_operations_use_typed_unsupported_errors() {
        let failure = message(
            1,
            ProcessWindowMessage {
                message: 1,
                wparam: 0,
                lparam: 0,
            },
        )
        .unwrap_err();
        assert_eq!(failure.code, "process_window_unsupported");
        assert_eq!(failure.cause, Some("unsupported"));
    }
}
