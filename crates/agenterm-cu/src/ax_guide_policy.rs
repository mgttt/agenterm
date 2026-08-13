//! When to show the Accessibility card and when to reopen Settings.
//!
//! Pure time + flags. No AppKit. Seconds are monotonic from an arbitrary origin.
//!
//! The important product rule: never reopen Settings on a short timer while the
//! user is already in that pane. Reopening steals the click they need to flip
//! the switch.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GuideState {
    pub was_trusted: bool,
    pub visible: bool,
    last_settings_s: Option<u64>,
    last_prompt_s: Option<u64>,
    closed_at_s: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickOut {
    pub show: bool,
    pub open_settings: bool,
    pub prompt_system: bool,
}

pub const SETTINGS_COOLDOWN_S: u64 = 45;
pub const RESHOW_AFTER_CLOSE_S: u64 = 6;

impl GuideState {
    pub fn tick(&mut self, now_s: u64, trusted: bool, settings_front: bool) -> TickOut {
        if trusted {
            self.was_trusted = true;
            self.visible = false;
            self.closed_at_s = None;
            return TickOut::default();
        }

        let revoked = self.was_trusted;
        self.was_trusted = false;

        let prompt_system = revoked || self.last_prompt_s.is_none();
        if prompt_system {
            self.last_prompt_s = Some(now_s);
        }

        let settings_due = match self.last_settings_s {
            None => true,
            Some(t) => now_s.saturating_sub(t) >= SETTINGS_COOLDOWN_S && !settings_front,
        };
        let open_settings = revoked || settings_due;
        if open_settings {
            self.last_settings_s = Some(now_s);
        }

        let show = if self.visible {
            true
        } else if let Some(closed) = self.closed_at_s {
            now_s.saturating_sub(closed) >= RESHOW_AFTER_CLOSE_S
        } else {
            true
        };
        if show {
            self.visible = true;
            self.closed_at_s = None;
        }

        TickOut {
            show,
            open_settings,
            prompt_system,
        }
    }

    pub fn note_closed(&mut self, now_s: u64) {
        self.visible = false;
        self.closed_at_s = Some(now_s);
    }

    /// Hotkey failed because AX is off: drop cooldowns and help again now.
    pub fn force_help(&mut self) {
        self.last_settings_s = None;
        self.last_prompt_s = None;
        self.closed_at_s = None;
        self.visible = false;
        self.was_trusted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_stays_silent() {
        let mut s = GuideState::default();
        let out = s.tick(0, true, false);
        assert_eq!(out, TickOut::default());
        assert!(s.was_trusted);
        assert!(!s.visible);
    }

    #[test]
    fn first_untrusted_opens_settings_and_system_prompt() {
        let mut s = GuideState::default();
        let out = s.tick(0, false, false);
        assert_eq!(
            out,
            TickOut {
                show: true,
                open_settings: true,
                prompt_system: true,
            }
        );
    }

    #[test]
    fn does_not_spam_while_user_is_in_settings() {
        let mut s = GuideState::default();
        s.tick(0, false, false);
        let out = s.tick(10, false, true);
        assert_eq!(
            out,
            TickOut {
                show: true,
                open_settings: false,
                prompt_system: false,
            }
        );
        let still = s.tick(50, false, true);
        assert!(!still.open_settings);
        assert!(!still.prompt_system);
        assert!(still.show);
    }

    #[test]
    fn reopens_settings_after_cooldown_if_user_left() {
        let mut s = GuideState::default();
        s.tick(0, false, false);
        let out = s.tick(45, false, false);
        assert!(out.open_settings);
        assert!(!out.prompt_system);
    }

    #[test]
    fn closed_card_comes_back_after_delay() {
        let mut s = GuideState::default();
        s.tick(0, false, true);
        s.note_closed(1);
        let too_soon = s.tick(4, false, true);
        assert!(!too_soon.show);
        let back = s.tick(8, false, true);
        assert!(back.show);
        assert!(!back.open_settings);
    }

    #[test]
    fn revoke_helps_immediately() {
        let mut s = GuideState::default();
        s.tick(0, true, false);
        s.tick(3, true, false);
        let out = s.tick(4, false, false);
        assert_eq!(
            out,
            TickOut {
                show: true,
                open_settings: true,
                prompt_system: true,
            }
        );
    }

    #[test]
    fn force_help_drops_cooldown() {
        let mut s = GuideState::default();
        s.tick(0, false, false);
        s.force_help();
        let out = s.tick(1, false, false);
        assert!(out.open_settings);
        assert!(out.prompt_system);
        assert!(out.show);
    }

    #[test]
    fn grant_hides_card() {
        let mut s = GuideState::default();
        s.tick(0, false, false);
        let out = s.tick(2, true, true);
        assert_eq!(out, TickOut::default());
        assert!(!s.visible);
        assert!(s.was_trusted);
    }
}
