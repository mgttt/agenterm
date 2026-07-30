#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCursorAppearance {
    pub(crate) shape: TerminalCursorShape,
    pub(crate) blinking: bool,
}

impl Default for TerminalCursorAppearance {
    fn default() -> Self {
        Self {
            shape: TerminalCursorShape::Block,
            blinking: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
enum ParseState {
    #[default]
    Ground,
    Escape,
    Csi {
        parameter: u16,
        has_parameter: bool,
        space: bool,
        valid: bool,
    },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DecscusrTracker {
    state: ParseState,
    appearance: TerminalCursorAppearance,
}

impl DecscusrTracker {
    pub(crate) const fn appearance(self) -> TerminalCursorAppearance {
        self.appearance
    }

    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.feed_byte(byte);
        }
    }

    fn feed_byte(&mut self, byte: u8) {
        self.state = match self.state {
            ParseState::Ground => match byte {
                0x1b => ParseState::Escape,
                0x9b => Self::new_csi(),
                _ => ParseState::Ground,
            },
            ParseState::Escape => match byte {
                b'[' => Self::new_csi(),
                0x1b => ParseState::Escape,
                _ => ParseState::Ground,
            },
            ParseState::Csi {
                mut parameter,
                mut has_parameter,
                mut space,
                mut valid,
            } => {
                if byte == 0x1b {
                    return self.state = ParseState::Escape;
                }
                if byte.is_ascii_digit() && !space {
                    has_parameter = true;
                    parameter = parameter
                        .saturating_mul(10)
                        .saturating_add(u16::from(byte - b'0'));
                } else if byte == b' ' && !space {
                    space = true;
                } else if byte == b'q' {
                    if valid && space {
                        self.apply_parameter(if has_parameter { parameter } else { 0 });
                    }
                    return self.state = ParseState::Ground;
                } else if (0x40..=0x7e).contains(&byte) {
                    return self.state = ParseState::Ground;
                } else {
                    valid = false;
                }
                ParseState::Csi {
                    parameter,
                    has_parameter,
                    space,
                    valid,
                }
            }
        };
    }

    const fn new_csi() -> ParseState {
        ParseState::Csi {
            parameter: 0,
            has_parameter: false,
            space: false,
            valid: true,
        }
    }

    fn apply_parameter(&mut self, parameter: u16) {
        self.appearance = match parameter {
            0 | 1 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Block,
                blinking: true,
            },
            2 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Block,
                blinking: false,
            },
            3 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Underline,
                blinking: true,
            },
            4 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Underline,
                blinking: false,
            },
            5 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Bar,
                blinking: true,
            },
            6 => TerminalCursorAppearance {
                shape: TerminalCursorShape::Bar,
                blinking: false,
            },
            _ => self.appearance,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{DecscusrTracker, TerminalCursorAppearance, TerminalCursorShape};

    #[test]
    fn decscusr_tracks_every_shape_and_blink_variant() {
        let cases = [
            (0, TerminalCursorShape::Block, true),
            (1, TerminalCursorShape::Block, true),
            (2, TerminalCursorShape::Block, false),
            (3, TerminalCursorShape::Underline, true),
            (4, TerminalCursorShape::Underline, false),
            (5, TerminalCursorShape::Bar, true),
            (6, TerminalCursorShape::Bar, false),
        ];
        for (parameter, shape, blinking) in cases {
            let mut tracker = DecscusrTracker::default();
            tracker.feed(format!("\u{1b}[{parameter} q").as_bytes());
            assert_eq!(
                tracker.appearance(),
                TerminalCursorAppearance { shape, blinking }
            );
        }
    }

    #[test]
    fn decscusr_survives_every_chunk_boundary_and_ignores_other_csi() {
        let sequence = b"before\x1b[5 qafter";
        for split in 0..=sequence.len() {
            let mut tracker = DecscusrTracker::default();
            tracker.feed(&sequence[..split]);
            tracker.feed(&sequence[split..]);
            assert_eq!(
                tracker.appearance(),
                TerminalCursorAppearance {
                    shape: TerminalCursorShape::Bar,
                    blinking: true,
                }
            );
        }

        let mut tracker = DecscusrTracker::default();
        tracker.feed(b"\x1b[31m\x1b[?25l\x1b[99 q");
        assert_eq!(tracker.appearance(), TerminalCursorAppearance::default());
        tracker.feed(b"\x9b4 q");
        assert_eq!(
            tracker.appearance(),
            TerminalCursorAppearance {
                shape: TerminalCursorShape::Underline,
                blinking: false,
            }
        );
    }
}
