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
}

impl FocusTransitionGate {
    pub(crate) const fn blocked(self) -> bool {
        self.window_close_pending
            || self.settings_open
            || self.new_terminal_open
            || self.tab_editor_open
            || self.close_confirmation_open
            || self.cwd_editor_open
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
        ApplicationMouseMode, FocusDirection, FocusState, FocusSurface, FocusTransitionGate,
        MouseDelivery, ScrollbarThumbDrag, WheelAccumulator, WheelTarget, focus_surface_navigation,
        mouse_delivery, route_wheel, sidebar_scroll_offset_for_thumb_top,
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
}
