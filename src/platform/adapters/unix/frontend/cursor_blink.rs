//! Unix frontend cursor blink state.

use std::time::{Duration, Instant};

/// Caret blink half-period, owned by shared input policy because macOS and each
/// Linux desktop expose their own user-visible setting for it.
pub(super) fn cursor_blink_interval() -> Duration {
    Duration::from_millis(crate::platform::caret_blink_interval_ms())
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CursorBlink {
    visible: bool,
    next_toggle: Instant,
}

impl CursorBlink {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            visible: true,
            next_toggle: now + cursor_blink_interval(),
        }
    }

    pub(super) const fn visible(self) -> bool {
        self.visible
    }

    pub(super) const fn next_toggle(self) -> Instant {
        self.next_toggle
    }

    pub(super) fn reset(&mut self, now: Instant) -> bool {
        let changed = !self.visible;
        self.visible = true;
        self.next_toggle = now + cursor_blink_interval();
        changed
    }

    pub(super) fn tick(&mut self, now: Instant) -> bool {
        if now < self.next_toggle {
            return false;
        }
        self.visible = !self.visible;
        self.next_toggle = now + cursor_blink_interval();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{CursorBlink, cursor_blink_interval};
    use std::time::Instant;

    #[test]
    fn cursor_blink_toggles_on_deadline_and_reset_forces_visible() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);

        assert!(blink.visible());
        assert!(!blink.tick(start + cursor_blink_interval() / 2));
        assert!(blink.tick(start + cursor_blink_interval()));
        assert!(!blink.visible());
        assert!(blink.reset(start + cursor_blink_interval()));
        assert!(blink.visible());
        assert_eq!(blink.next_toggle(), start + cursor_blink_interval() * 2);
    }
}
