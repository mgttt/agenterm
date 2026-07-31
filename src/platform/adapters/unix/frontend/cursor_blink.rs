//! Unix frontend cursor blink state.

use std::time::{Duration, Instant};

pub(super) const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(600);

#[derive(Clone, Copy, Debug)]
pub(super) struct CursorBlink {
    visible: bool,
    next_toggle: Instant,
}

impl CursorBlink {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            visible: true,
            next_toggle: now + CURSOR_BLINK_INTERVAL,
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
        self.next_toggle = now + CURSOR_BLINK_INTERVAL;
        changed
    }

    pub(super) fn tick(&mut self, now: Instant) -> bool {
        if now < self.next_toggle {
            return false;
        }
        self.visible = !self.visible;
        self.next_toggle = now + CURSOR_BLINK_INTERVAL;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{CURSOR_BLINK_INTERVAL, CursorBlink};
    use std::time::Instant;

    #[test]
    fn cursor_blink_toggles_on_deadline_and_reset_forces_visible() {
        let start = Instant::now();
        let mut blink = CursorBlink::new(start);

        assert!(blink.visible());
        assert!(!blink.tick(start + CURSOR_BLINK_INTERVAL / 2));
        assert!(blink.tick(start + CURSOR_BLINK_INTERVAL));
        assert!(!blink.visible());
        assert!(blink.reset(start + CURSOR_BLINK_INTERVAL));
        assert!(blink.visible());
        assert_eq!(blink.next_toggle(), start + CURSOR_BLINK_INTERVAL * 2);
    }
}
