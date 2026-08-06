//! Product interaction semantics shared by platform frontend adapters.
//!
//! This layer owns cross-platform UX policy for focus navigation and wheel
//! accumulation. Platform adapters map native events into these types and back
//! so the same visible behavior is not reimplemented per host.

use crate::ui_geometry::{TerminalScrollbarGeometry, WHEEL_DELTA};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusSurface {
    Terminal,
    Composer,
    Sidebar,
}

impl FocusSurface {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Composer => "composer",
            Self::Sidebar => "tabs",
        }
    }

    pub(crate) fn from_ipc(value: &str) -> Result<Self, String> {
        match value {
            "terminal" => Ok(Self::Terminal),
            "composer" => Ok(Self::Composer),
            "tabs" | "sidebar" => Ok(Self::Sidebar),
            other => Err(format!("unknown focus surface: {other}")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusDirection {
    Up,
    Down,
    Left,
    Right,
}

impl FocusDirection {
    pub(crate) const fn from_virtual_key_code(key: u32) -> Option<Self> {
        match key {
            0x25 => Some(Self::Left),
            0x26 => Some(Self::Up),
            0x27 => Some(Self::Right),
            0x28 => Some(Self::Down),
            _ => None,
        }
    }
}

pub(crate) fn focus_surface_navigation(
    source: FocusSurface,
    direction: FocusDirection,
    control: bool,
    shift: bool,
    alt: bool,
) -> Option<FocusSurface> {
    if !control || shift || alt {
        return None;
    }
    match (source, direction) {
        (FocusSurface::Terminal, FocusDirection::Down) => Some(FocusSurface::Composer),
        (FocusSurface::Composer, FocusDirection::Up) => Some(FocusSurface::Terminal),
        (FocusSurface::Terminal, FocusDirection::Left) => Some(FocusSurface::Sidebar),
        (FocusSurface::Sidebar, FocusDirection::Right) => Some(FocusSurface::Terminal),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WheelAccumulator {
    remainder: i32,
}

impl WheelAccumulator {
    pub(crate) fn push(&mut self, units: i32) -> i32 {
        self.remainder += units;
        let notches = self.remainder / WHEEL_DELTA;
        self.remainder %= WHEEL_DELTA;
        notches
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FocusTransitionGate {
    pub(crate) window_close_pending: bool,
    pub(crate) settings_open: bool,
    pub(crate) new_terminal_open: bool,
    pub(crate) tab_editor_open: bool,
    pub(crate) close_confirmation_open: bool,
    pub(crate) cwd_editor_open: bool,
    pub(crate) instance_picker_open: bool,
    pub(crate) server_new_open: bool,
    pub(crate) server_close_pending: bool,
}

impl FocusTransitionGate {
    pub(crate) const fn blocked(self) -> bool {
        self.full_modal_blocked() || self.tab_editor_open || self.cwd_editor_open
    }

    /// Full-screen modal surfaces that intercept terminal and workspace input.
    ///
    /// Inline tab/CWD editors remain reachable while they are open, so they
    /// deliberately do not count as full modal blocking.
    pub(crate) const fn full_modal_blocked(self) -> bool {
        self.window_close_pending
            || self.settings_open
            || self.new_terminal_open
            || self.close_confirmation_open
            || self.instance_picker_open
            || self.server_new_open
            || self.server_close_pending
    }

    pub(crate) const fn modal_entry_blocked(self, surface: ModalSurface) -> bool {
        match surface {
            ModalSurface::WindowClose => self.window_close_pending,
            ModalSurface::Settings
            | ModalSurface::NewTerminal
            | ModalSurface::InstancePicker
            | ModalSurface::ServerNew
            | ModalSurface::ServerClose => self.full_modal_blocked(),
            ModalSurface::TabClose => self.full_modal_blocked(),
            ModalSurface::CwdEditor => self.blocked(),
        }
    }

    pub(crate) const fn workspace_controls_visible(self) -> bool {
        !self.window_close_pending
            && !self.settings_open
            && !self.new_terminal_open
            && !self.close_confirmation_open
            && !self.instance_picker_open
            && !self.server_new_open
            && !self.server_close_pending
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModalSurface {
    WindowClose,
    Settings,
    NewTerminal,
    CwdEditor,
    TabClose,
    InstancePicker,
    ServerNew,
    ServerClose,
}

impl ModalSurface {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::WindowClose => "window-close",
            Self::Settings => "settings",
            Self::NewTerminal => "new-terminal",
            Self::CwdEditor => "cwd-editor",
            Self::TabClose => "tab-close",
            Self::InstancePicker => "instance-picker",
            Self::ServerNew => "server-new",
            Self::ServerClose => "server-close",
        }
    }
}

pub(crate) fn modal_surface_from_gate(gate: FocusTransitionGate) -> Option<ModalSurface> {
    if gate.window_close_pending {
        Some(ModalSurface::WindowClose)
    } else if gate.server_close_pending {
        Some(ModalSurface::ServerClose)
    } else if gate.server_new_open {
        Some(ModalSurface::ServerNew)
    } else if gate.instance_picker_open {
        Some(ModalSurface::InstancePicker)
    } else if gate.settings_open {
        Some(ModalSurface::Settings)
    } else if gate.new_terminal_open {
        Some(ModalSurface::NewTerminal)
    } else if gate.cwd_editor_open {
        Some(ModalSurface::CwdEditor)
    } else if gate.close_confirmation_open {
        Some(ModalSurface::TabClose)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowCloseRequest {
    AlreadyOpen,
    CancelLiveClose,
    Prepare,
}

pub(crate) const fn window_close_request(gate: FocusTransitionGate) -> WindowCloseRequest {
    if gate.window_close_pending {
        WindowCloseRequest::AlreadyOpen
    } else if gate.close_confirmation_open {
        WindowCloseRequest::CancelLiveClose
    } else {
        WindowCloseRequest::Prepare
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CancelTarget {
    WindowClose,
    LiveTabClose,
    Settings,
    NewTerminal,
    CwdEditor,
    TabEditor,
    InstancePicker,
    None,
}

pub(crate) const fn cancel_target(gate: FocusTransitionGate) -> CancelTarget {
    if gate.window_close_pending {
        CancelTarget::WindowClose
    } else if gate.instance_picker_open {
        CancelTarget::InstancePicker
    } else if gate.close_confirmation_open {
        CancelTarget::LiveTabClose
    } else if gate.settings_open {
        CancelTarget::Settings
    } else if gate.new_terminal_open {
        CancelTarget::NewTerminal
    } else if gate.cwd_editor_open {
        CancelTarget::CwdEditor
    } else if gate.tab_editor_open {
        CancelTarget::TabEditor
    } else {
        CancelTarget::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmTarget {
    WindowClose,
    LiveTabClose,
    InstancePicker,
    None,
}

pub(crate) const fn confirm_target(gate: FocusTransitionGate) -> ConfirmTarget {
    if gate.window_close_pending {
        ConfirmTarget::WindowClose
    } else if gate.instance_picker_open {
        ConfirmTarget::InstancePicker
    } else if gate.close_confirmation_open {
        ConfirmTarget::LiveTabClose
    } else {
        ConfirmTarget::None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FocusState {
    surface: FocusSurface,
    gate: FocusTransitionGate,
}

impl FocusState {
    pub(crate) const fn new(surface: FocusSurface, gate: FocusTransitionGate) -> Self {
        Self { surface, gate }
    }

    pub(crate) const fn surface(self) -> FocusSurface {
        self.surface
    }

    pub(crate) fn transition(&mut self, target: FocusSurface) -> bool {
        if self.gate.blocked() {
            return false;
        }
        self.surface = target;
        true
    }

    pub(crate) fn navigate(
        &self,
        direction: FocusDirection,
        control: bool,
        shift: bool,
        alt: bool,
    ) -> Option<FocusSurface> {
        if self.gate.blocked() {
            return None;
        }
        focus_surface_navigation(self.surface, direction, control, shift, alt)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelTarget {
    Sidebar,
    Terminal,
    Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApplicationMouseMode {
    None,
    Press,
    PressRelease,
    ButtonMotion,
    AnyMotion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseDelivery {
    LocalSelection,
    Application,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseReportEncoding {
    Default,
    Sgr,
    Utf8,
}

pub(crate) fn mouse_protocol_mode_name(mode: ApplicationMouseMode) -> &'static str {
    match mode {
        ApplicationMouseMode::None => "none",
        ApplicationMouseMode::Press => "press",
        ApplicationMouseMode::PressRelease => "press-release",
        ApplicationMouseMode::ButtonMotion => "button-motion",
        ApplicationMouseMode::AnyMotion => "any-motion",
    }
}

pub(crate) fn mouse_protocol_mode_from_str(value: &str) -> ApplicationMouseMode {
    match value {
        "press" => ApplicationMouseMode::Press,
        "press-release" => ApplicationMouseMode::PressRelease,
        "button-motion" => ApplicationMouseMode::ButtonMotion,
        "any-motion" => ApplicationMouseMode::AnyMotion,
        _ => ApplicationMouseMode::None,
    }
}

pub(crate) fn mouse_report_encoding_name(encoding: MouseReportEncoding) -> &'static str {
    match encoding {
        MouseReportEncoding::Default => "default",
        MouseReportEncoding::Sgr => "sgr",
        MouseReportEncoding::Utf8 => "utf8",
    }
}

pub(crate) fn mouse_report_encoding_from_str(value: &str) -> MouseReportEncoding {
    match value {
        "sgr" => MouseReportEncoding::Sgr,
        "utf8" => MouseReportEncoding::Utf8,
        _ => MouseReportEncoding::Default,
    }
}

/// Encodes one xterm mouse report for the PTY in the encoding the running
/// application negotiated. Coordinates are zero-based grid cells; the classic
/// encodings cannot express release button identity or cells past their byte
/// range, so those degrade exactly like xterm (button 3, event dropped).
pub(crate) fn mouse_report_bytes(
    encoding: MouseReportEncoding,
    code: u8,
    column: u16,
    row: u16,
    pressed: bool,
) -> Option<Vec<u8>> {
    match encoding {
        MouseReportEncoding::Sgr => {
            let suffix = if pressed { 'M' } else { 'm' };
            Some(
                format!(
                    "\x1b[<{code};{};{}{suffix}",
                    u32::from(column) + 1,
                    u32::from(row) + 1
                )
                .into_bytes(),
            )
        }
        MouseReportEncoding::Default => {
            let code = if pressed { code } else { (code & !0b11) | 3 };
            let column = u8::try_from(column.checked_add(33)?).ok()?;
            let row = u8::try_from(row.checked_add(33)?).ok()?;
            Some(vec![0x1b, b'[', b'M', 32 + code, column, row])
        }
        MouseReportEncoding::Utf8 => {
            let code = if pressed { code } else { (code & !0b11) | 3 };
            let mut bytes = vec![0x1b, b'[', b'M'];
            let mut push = |scalar: u32| {
                let character = char::from_u32(scalar)?;
                let mut buffer = [0u8; 4];
                bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                Some(())
            };
            push(u32::from(code) + 32)?;
            push(u32::from(column) + 33)?;
            push(u32::from(row) + 33)?;
            Some(bytes)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MouseReportInput {
    pub mode: ApplicationMouseMode,
    pub encoding: MouseReportEncoding,
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
    pub scrolled_back: bool,
    pub motion: bool,
    pub dragging: bool,
    pub pressed: bool,
    pub button: Option<u8>,
    pub current_button: Option<u8>,
    pub current_cell: Option<(u16, u16)>,
    pub column: u16,
    pub row: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MouseReportOutcome {
    LocalSelection,
    Deduplicated,
    Send(Vec<u8>),
}

pub(crate) fn mouse_report_outcome(input: MouseReportInput) -> MouseReportOutcome {
    let MouseReportInput {
        mode,
        encoding,
        shift,
        alt,
        control,
        scrolled_back,
        motion,
        dragging,
        pressed,
        button,
        current_button,
        current_cell,
        column,
        row,
    } = input;
    if mouse_delivery(mode, shift, scrolled_back, motion, dragging, pressed)
        != MouseDelivery::Application
    {
        return MouseReportOutcome::LocalSelection;
    }
    if motion && current_cell == Some((column, row)) {
        return MouseReportOutcome::Deduplicated;
    }
    let mut code = match button.or(current_button) {
        Some(code) => code,
        None if motion => 3,
        None => return MouseReportOutcome::LocalSelection,
    };
    if motion {
        code |= 32;
    }
    if alt {
        code |= 8;
    }
    if control {
        code |= 16;
    }
    mouse_report_bytes(encoding, code, column, row, pressed)
        .map_or(MouseReportOutcome::LocalSelection, MouseReportOutcome::Send)
}
pub(crate) fn system_menu_clipboard_state(
    edit_focus: bool,
    terminal_ready: bool,
    selection_nonempty: bool,
    clipboard_has_text: bool,
) -> (bool, bool) {
    if edit_focus {
        return (true, clipboard_has_text);
    }
    (
        terminal_ready && selection_nonempty,
        terminal_ready && clipboard_has_text,
    )
}

pub(crate) fn mouse_delivery(
    mode: ApplicationMouseMode,
    shift_override: bool,
    scrolled_back: bool,
    motion: bool,
    dragging: bool,
    pressed: bool,
) -> MouseDelivery {
    if shift_override || scrolled_back || mode == ApplicationMouseMode::None {
        return MouseDelivery::LocalSelection;
    }
    let reportable = match mode {
        ApplicationMouseMode::None => false,
        ApplicationMouseMode::Press => pressed && !motion,
        ApplicationMouseMode::PressRelease => !motion,
        ApplicationMouseMode::ButtonMotion => !motion || dragging,
        ApplicationMouseMode::AnyMotion => true,
    };
    if reportable {
        MouseDelivery::Application
    } else {
        MouseDelivery::LocalSelection
    }
}
pub(crate) fn route_wheel(sidebar_hit: bool, terminal_hit: bool) -> WheelTarget {
    if sidebar_hit {
        WheelTarget::Sidebar
    } else if terminal_hit {
        WheelTarget::Terminal
    } else {
        WheelTarget::Ignored
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScrollbarThumbDrag {
    thumb_grab_offset: i32,
}

impl ScrollbarThumbDrag {
    pub(crate) fn begin(pointer_y: i32, thumb_top: i32) -> Self {
        Self {
            thumb_grab_offset: pointer_y - thumb_top,
        }
    }

    pub(crate) fn thumb_top(self, pointer_y: i32) -> i32 {
        pointer_y - self.thumb_grab_offset
    }
}

pub(crate) fn sidebar_scroll_offset_for_thumb_top(
    geometry: TerminalScrollbarGeometry,
    thumb_top: i32,
    maximum: usize,
) -> usize {
    let travel = geometry.track.height() - geometry.thumb.height();
    if maximum == 0 || travel <= 0 {
        return 0;
    }
    let top = thumb_top.clamp(
        geometry.track.top,
        geometry.track.bottom - geometry.thumb.height(),
    );
    ((i64::from(top - geometry.track.top) * maximum as i64 + i64::from(travel) / 2)
        / i64::from(travel)) as usize
}
#[cfg(test)]
mod tests {
    use super::{
        ApplicationMouseMode, CancelTarget, ConfirmTarget, FocusDirection, FocusState,
        FocusSurface, FocusTransitionGate, ModalSurface, MouseDelivery, MouseReportEncoding,
        MouseReportInput, MouseReportOutcome, ScrollbarThumbDrag, WheelAccumulator, WheelTarget,
        WindowCloseRequest, cancel_target, confirm_target, focus_surface_navigation,
        modal_surface_from_gate, mouse_delivery, mouse_report_outcome, route_wheel,
        sidebar_scroll_offset_for_thumb_top, system_menu_clipboard_state, window_close_request,
    };

    #[test]
    fn focus_transition_gate_blocks_each_modal_state() {
        let idle = FocusTransitionGate::default();
        assert!(!idle.blocked());
        assert!(
            FocusTransitionGate {
                window_close_pending: true,
                ..idle
            }
            .blocked()
        );
        assert!(
            FocusTransitionGate {
                settings_open: true,
                ..idle
            }
            .blocked()
        );
        assert!(
            FocusTransitionGate {
                new_terminal_open: true,
                ..idle
            }
            .blocked()
        );
        assert!(
            FocusTransitionGate {
                tab_editor_open: true,
                ..idle
            }
            .blocked()
        );
        assert!(
            FocusTransitionGate {
                close_confirmation_open: true,
                ..idle
            }
            .blocked()
        );
        assert!(
            FocusTransitionGate {
                cwd_editor_open: true,
                ..idle
            }
            .blocked()
        );
    }

    #[test]
    fn full_modal_blocked_excludes_inline_editors() {
        let idle = FocusTransitionGate::default();
        assert!(!idle.full_modal_blocked());
        for gate in [
            FocusTransitionGate {
                window_close_pending: true,
                ..idle
            },
            FocusTransitionGate {
                settings_open: true,
                ..idle
            },
            FocusTransitionGate {
                new_terminal_open: true,
                ..idle
            },
            FocusTransitionGate {
                close_confirmation_open: true,
                ..idle
            },
        ] {
            assert!(gate.full_modal_blocked());
        }
        assert!(
            !FocusTransitionGate {
                tab_editor_open: true,
                ..idle
            }
            .full_modal_blocked()
        );
        assert!(
            !FocusTransitionGate {
                cwd_editor_open: true,
                ..idle
            }
            .full_modal_blocked()
        );
        assert!(
            FocusTransitionGate {
                tab_editor_open: true,
                cwd_editor_open: true,
                ..idle
            }
            .blocked()
        );
    }

    #[test]
    fn modal_entry_policy_is_shared_across_full_and_inline_surfaces() {
        let idle = FocusTransitionGate::default();
        assert!(!idle.modal_entry_blocked(ModalSurface::NewTerminal));
        assert!(!idle.modal_entry_blocked(ModalSurface::Settings));
        assert!(!idle.modal_entry_blocked(ModalSurface::CwdEditor));
        assert!(!idle.modal_entry_blocked(ModalSurface::TabClose));

        let full = FocusTransitionGate {
            close_confirmation_open: true,
            ..idle
        };
        assert!(full.modal_entry_blocked(ModalSurface::NewTerminal));
        assert!(full.modal_entry_blocked(ModalSurface::Settings));
        assert!(full.modal_entry_blocked(ModalSurface::CwdEditor));
        assert!(full.modal_entry_blocked(ModalSurface::TabClose));

        let inline = FocusTransitionGate {
            tab_editor_open: true,
            ..idle
        };
        assert!(!inline.modal_entry_blocked(ModalSurface::NewTerminal));
        assert!(!inline.modal_entry_blocked(ModalSurface::Settings));
        assert!(inline.modal_entry_blocked(ModalSurface::CwdEditor));
        assert!(!inline.modal_entry_blocked(ModalSurface::TabClose));
        assert!(
            FocusTransitionGate {
                window_close_pending: true,
                ..idle
            }
            .modal_entry_blocked(ModalSurface::WindowClose)
        );
    }

    #[test]
    fn cancel_target_uses_shared_modal_priority() {
        let idle = FocusTransitionGate::default();
        assert_eq!(cancel_target(idle), CancelTarget::None);
        assert_eq!(
            cancel_target(FocusTransitionGate {
                window_close_pending: true,
                close_confirmation_open: true,
                settings_open: true,
                new_terminal_open: true,
                cwd_editor_open: true,
                tab_editor_open: true,
                instance_picker_open: false,
                server_new_open: false,
                server_close_pending: false,
            }),
            CancelTarget::WindowClose
        );
        assert_eq!(
            cancel_target(FocusTransitionGate {
                close_confirmation_open: true,
                settings_open: true,
                new_terminal_open: true,
                ..idle
            }),
            CancelTarget::LiveTabClose
        );
        assert_eq!(
            cancel_target(FocusTransitionGate {
                settings_open: true,
                new_terminal_open: true,
                ..idle
            }),
            CancelTarget::Settings
        );
        assert_eq!(
            cancel_target(FocusTransitionGate {
                new_terminal_open: true,
                cwd_editor_open: true,
                tab_editor_open: true,
                ..idle
            }),
            CancelTarget::NewTerminal
        );
        assert_eq!(
            cancel_target(FocusTransitionGate {
                cwd_editor_open: true,
                tab_editor_open: true,
                ..idle
            }),
            CancelTarget::CwdEditor
        );
        assert_eq!(
            cancel_target(FocusTransitionGate {
                tab_editor_open: true,
                ..idle
            }),
            CancelTarget::TabEditor
        );
    }

    #[test]
    fn confirm_target_uses_shared_modal_priority() {
        let idle = FocusTransitionGate::default();
        assert_eq!(confirm_target(idle), ConfirmTarget::None);
        assert_eq!(
            confirm_target(FocusTransitionGate {
                window_close_pending: true,
                close_confirmation_open: true,
                ..idle
            }),
            ConfirmTarget::WindowClose
        );
        assert_eq!(
            confirm_target(FocusTransitionGate {
                close_confirmation_open: true,
                settings_open: true,
                ..idle
            }),
            ConfirmTarget::LiveTabClose
        );
    }

    #[test]
    fn window_close_request_captures_live_close_cancel() {
        let idle = FocusTransitionGate::default();
        assert_eq!(window_close_request(idle), WindowCloseRequest::Prepare);
        assert_eq!(
            window_close_request(FocusTransitionGate {
                window_close_pending: true,
                ..idle
            }),
            WindowCloseRequest::AlreadyOpen
        );
        assert_eq!(
            window_close_request(FocusTransitionGate {
                close_confirmation_open: true,
                ..idle
            }),
            WindowCloseRequest::CancelLiveClose
        );
        assert_eq!(
            window_close_request(FocusTransitionGate {
                settings_open: true,
                new_terminal_open: true,
                ..idle
            }),
            WindowCloseRequest::Prepare
        );
    }

    #[test]
    fn modal_surface_uses_single_priority_and_excludes_tab_editor() {
        let idle = FocusTransitionGate::default();
        assert_eq!(modal_surface_from_gate(idle), None);
        assert_eq!(
            modal_surface_from_gate(FocusTransitionGate {
                tab_editor_open: true,
                ..idle
            }),
            None
        );
        let gate = FocusTransitionGate {
            window_close_pending: true,
            settings_open: true,
            new_terminal_open: true,
            cwd_editor_open: true,
            close_confirmation_open: true,
            tab_editor_open: true,
            instance_picker_open: true,
                server_new_open: false,
                server_close_pending: false,
        };
        assert_eq!(
            modal_surface_from_gate(gate),
            Some(ModalSurface::WindowClose)
        );
        assert_eq!(
            modal_surface_from_gate(FocusTransitionGate {
                settings_open: true,
                new_terminal_open: true,
                ..idle
            }),
            Some(ModalSurface::Settings)
        );
        assert_eq!(
            modal_surface_from_gate(FocusTransitionGate {
                new_terminal_open: true,
                cwd_editor_open: true,
                ..idle
            }),
            Some(ModalSurface::NewTerminal)
        );
        assert_eq!(
            modal_surface_from_gate(FocusTransitionGate {
                cwd_editor_open: true,
                close_confirmation_open: true,
                ..idle
            }),
            Some(ModalSurface::CwdEditor)
        );
        assert_eq!(
            modal_surface_from_gate(FocusTransitionGate {
                close_confirmation_open: true,
                ..idle
            }),
            Some(ModalSurface::TabClose)
        );
    }

    #[test]
    fn focus_surface_canonical_names_and_ipc_aliases_round_trip() {
        for (surface, name) in [
            (FocusSurface::Terminal, "terminal"),
            (FocusSurface::Composer, "composer"),
            (FocusSurface::Sidebar, "tabs"),
        ] {
            assert_eq!(surface.as_str(), name);
            assert_eq!(FocusSurface::from_ipc(name), Ok(surface));
        }
        assert_eq!(FocusSurface::from_ipc("sidebar"), Ok(FocusSurface::Sidebar));
        assert_eq!(
            FocusSurface::from_ipc("settings"),
            Err("unknown focus surface: settings".to_owned())
        );
    }

    #[test]
    fn workspace_controls_visibility_keeps_cwd_and_tab_editors_but_hides_full_modals() {
        let idle = FocusTransitionGate::default();
        assert!(idle.workspace_controls_visible());
        assert!(
            FocusTransitionGate {
                cwd_editor_open: true,
                ..idle
            }
            .workspace_controls_visible()
        );
        assert!(
            FocusTransitionGate {
                tab_editor_open: true,
                ..idle
            }
            .workspace_controls_visible()
        );
        for gate in [
            FocusTransitionGate {
                window_close_pending: true,
                ..idle
            },
            FocusTransitionGate {
                settings_open: true,
                ..idle
            },
            FocusTransitionGate {
                new_terminal_open: true,
                ..idle
            },
            FocusTransitionGate {
                close_confirmation_open: true,
                ..idle
            },
        ] {
            assert!(!gate.workspace_controls_visible());
        }
    }

    #[test]
    fn focus_state_applies_gate_to_navigation_and_transition() {
        let mut idle = FocusState::new(FocusSurface::Terminal, FocusTransitionGate::default());
        assert_eq!(
            idle.navigate(FocusDirection::Down, true, false, false),
            Some(FocusSurface::Composer)
        );
        assert_eq!(idle.surface, FocusSurface::Terminal);
        assert!(idle.transition(FocusSurface::Sidebar));
        assert_eq!(idle.surface, FocusSurface::Sidebar);

        let mut blocked = FocusState::new(
            FocusSurface::Terminal,
            FocusTransitionGate {
                settings_open: true,
                ..FocusTransitionGate::default()
            },
        );
        assert_eq!(
            blocked.navigate(FocusDirection::Down, true, false, false),
            None
        );
        assert!(!blocked.transition(FocusSurface::Composer));
        assert_eq!(blocked.surface, FocusSurface::Terminal);
        assert!(blocked.gate.blocked());
    }
    #[test]
    fn focus_navigation_requires_control_without_shift_or_alt() {
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Terminal,
                FocusDirection::Down,
                true,
                false,
                false,
            ),
            Some(FocusSurface::Composer)
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Terminal,
                FocusDirection::Down,
                false,
                false,
                false,
            ),
            None
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Terminal,
                FocusDirection::Down,
                true,
                true,
                false,
            ),
            None
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Terminal,
                FocusDirection::Down,
                true,
                false,
                true,
            ),
            None
        );
    }

    #[test]
    fn focus_navigation_matches_windows_remote_surface_map() {
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Composer,
                FocusDirection::Up,
                true,
                false,
                false,
            ),
            Some(FocusSurface::Terminal)
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Terminal,
                FocusDirection::Left,
                true,
                false,
                false,
            ),
            Some(FocusSurface::Sidebar)
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Sidebar,
                FocusDirection::Right,
                true,
                false,
                false,
            ),
            Some(FocusSurface::Terminal)
        );
        assert_eq!(
            focus_surface_navigation(
                FocusSurface::Composer,
                FocusDirection::Left,
                true,
                false,
                false,
            ),
            None
        );
    }

    #[test]
    fn virtual_key_codes_map_to_focus_directions() {
        assert_eq!(
            FocusDirection::from_virtual_key_code(0x26),
            Some(FocusDirection::Up)
        );
        assert_eq!(
            FocusDirection::from_virtual_key_code(0x28),
            Some(FocusDirection::Down)
        );
        assert_eq!(
            FocusDirection::from_virtual_key_code(0x25),
            Some(FocusDirection::Left)
        );
        assert_eq!(
            FocusDirection::from_virtual_key_code(0x27),
            Some(FocusDirection::Right)
        );
        assert_eq!(FocusDirection::from_virtual_key_code(0x0d), None);
    }

    #[test]
    fn wheel_accumulator_waits_for_full_notches_and_preserves_remainder() {
        let mut accumulator = WheelAccumulator::default();
        assert_eq!(accumulator.push(40), 0);
        assert_eq!(accumulator.push(40), 0);
        assert_eq!(accumulator.push(40), 1);
        assert_eq!(accumulator.push(-40), 0);
        assert_eq!(accumulator.push(-40), 0);
        assert_eq!(accumulator.push(-40), -1);
    }

    #[test]
    fn mouse_delivery_follows_xterm_protocol_phases() {
        use ApplicationMouseMode as Mode;
        assert_eq!(
            mouse_delivery(Mode::None, false, false, false, false, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(Mode::Press, false, false, false, false, true),
            MouseDelivery::Application
        );
        assert_eq!(
            mouse_delivery(Mode::Press, false, false, true, false, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(Mode::PressRelease, false, false, true, true, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(Mode::ButtonMotion, false, false, true, true, true),
            MouseDelivery::Application
        );
        assert_eq!(
            mouse_delivery(Mode::AnyMotion, false, false, true, false, false),
            MouseDelivery::Application
        );
        assert_eq!(
            mouse_delivery(Mode::Press, true, false, false, false, true),
            MouseDelivery::LocalSelection
        );
        assert_eq!(
            mouse_delivery(Mode::Press, false, true, false, false, true),
            MouseDelivery::LocalSelection
        );
    }

    #[test]
    fn wheel_route_sidebar_wins_over_terminal() {
        assert_eq!(route_wheel(true, true), WheelTarget::Sidebar);
        assert_eq!(route_wheel(false, true), WheelTarget::Terminal);
        assert_eq!(route_wheel(false, false), WheelTarget::Ignored);
    }

    #[test]
    fn scrollbar_thumb_drag_preserves_grab_offset() {
        let drag = ScrollbarThumbDrag::begin(25, 10);
        assert_eq!(drag.thumb_top(25), 10);
        assert_eq!(drag.thumb_top(40), 25);
    }

    #[test]
    fn sidebar_scroll_offset_clamps_thumb_top_into_track() {
        use crate::ui_geometry::{PixelRect, TerminalScrollbarGeometry};
        let geometry = TerminalScrollbarGeometry {
            track: PixelRect {
                left: 0,
                top: 0,
                right: 12,
                bottom: 100,
            },
            thumb: PixelRect {
                left: 2,
                top: 10,
                right: 10,
                bottom: 30,
            },
        };
        assert_eq!(sidebar_scroll_offset_for_thumb_top(geometry, 0, 100), 0);
        assert_eq!(sidebar_scroll_offset_for_thumb_top(geometry, 80, 100), 100);
    }

    #[test]
    fn system_menu_edit_focus_enables_copy_and_paste_follows_clipboard() {
        assert_eq!(
            system_menu_clipboard_state(true, false, false, false),
            (true, false)
        );
        assert_eq!(
            system_menu_clipboard_state(true, false, false, true),
            (true, true)
        );
    }

    #[test]
    fn system_menu_terminal_ready_requires_selection_and_clipboard() {
        assert_eq!(
            system_menu_clipboard_state(false, true, false, false),
            (false, false)
        );
        assert_eq!(
            system_menu_clipboard_state(false, true, true, false),
            (true, false)
        );
        assert_eq!(
            system_menu_clipboard_state(false, true, false, true),
            (false, true)
        );
        assert_eq!(
            system_menu_clipboard_state(false, true, true, true),
            (true, true)
        );
    }

    #[test]
    fn system_menu_terminal_not_ready_disables_copy_and_paste() {
        assert_eq!(
            system_menu_clipboard_state(false, false, true, true),
            (false, false)
        );
    }
    #[test]
    fn mouse_report_outcome_dedupes_motion_and_encodes_modifiers() {
        let deduplicated = MouseReportInput {
            mode: ApplicationMouseMode::AnyMotion,
            encoding: MouseReportEncoding::Sgr,
            shift: false,
            alt: true,
            control: true,
            scrolled_back: false,
            motion: true,
            dragging: true,
            pressed: true,
            button: None,
            current_button: Some(2),
            current_cell: Some((5, 5)),
            column: 5,
            row: 5,
        };
        assert_eq!(
            mouse_report_outcome(deduplicated),
            MouseReportOutcome::Deduplicated
        );
        let sent = MouseReportInput {
            column: 6,
            ..deduplicated
        };
        assert_eq!(
            mouse_report_outcome(sent),
            MouseReportOutcome::Send(vec![
                0x1b, b'[', b'<', b'5', b'8', b';', b'7', b';', b'6', b'M'
            ])
        );
        let local = MouseReportInput {
            mode: ApplicationMouseMode::Press,
            encoding: MouseReportEncoding::Sgr,
            shift: true,
            scrolled_back: false,
            alt: false,
            control: false,
            motion: false,
            dragging: false,
            pressed: true,
            button: Some(0),
            current_button: None,
            current_cell: None,
            column: 0,
            row: 0,
        };
        assert_eq!(
            mouse_report_outcome(local),
            MouseReportOutcome::LocalSelection
        );
    }
}
