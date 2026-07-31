//! PTY-neutral scalar types shared by native session adapters.

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalSize {
    pub(crate) rows: u16,
    pub(crate) cols: u16,
}

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ProcessId(pub(crate) u32);

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
impl ProcessId {
    pub(crate) fn new(raw: u32) -> Result<Self, InvalidProcessId> {
        if raw == 0 {
            return Err(InvalidProcessId(raw));
        }
        Ok(Self(raw))
    }
    pub(crate) const fn as_u32(self) -> u32 {
        self.0
    }
}

#[allow(dead_code)] // Consumed by the Unix PTY adapter only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidProcessId(pub(crate) u32);

impl std::fmt::Display for InvalidProcessId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid process id: {}", self.0)
    }
}
impl std::error::Error for InvalidProcessId {}
