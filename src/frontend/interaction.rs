//! Product interaction semantics shared by platform frontend adapters.
//!
//! This layer owns cross-platform UX policy for focus navigation and wheel
//! accumulation. Platform adapters map native events into these types and back
//! so the same visible behavior is not reimplemented per host.

use crate::ui_geometry::WHEEL_DELTA;

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

#[cfg(test)]
mod tests {
    use super::{FocusDirection, FocusSurface, WheelAccumulator, focus_surface_navigation};

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
}
