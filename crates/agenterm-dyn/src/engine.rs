//! Live-assembly engine (dyn.2): the time axis made real.
//!
//! Builds on the dyn.1 execution base ([`CodeBuffer`] + [`NameTable`]) by adding
//! the fifth piece the advice called out — a **patch table**. Names can be
//! *used before defined*: a `call` to an unknown name is staged with a
//! placeholder rel32 and parked in [`Engine::pending`]; when a later
//! [`Engine::assemble`] defines that name, the site is backpatched. Names also
//! persist across appends, so a second batch can `call` a name the first batch
//! defined.
//!
//! W^X is preserved throughout: each `assemble` re-enters the writable state to
//! append and backpatch, then flips to executable before returning. The buffer
//! is never writable and executable at once.
//!
//! Loud failure: a rel32 that cannot fit in `i32`, or exceeding the label /
//! pending-fixup caps, returns [`DynError::Exec`] rather than emitting a bad
//! displacement or silently dropping a fixup.

use crate::encoder::{Op, encode};
use crate::error::DynError;
use crate::exec::{CodeBuffer, NameTable};

/// Maximum distinct names the engine will register.
pub const MAX_LABELS: usize = 4_096;
/// Maximum unresolved forward references held at once.
pub const MAX_PENDING_FIXUPS: usize = 4_096;

/// A rel32 call site awaiting its target's definition.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    /// Buffer offset of the 4-byte rel32 field.
    site: usize,
    /// Name the site must reach.
    target: String,
}

/// A growable in-process code image with forward-reference patching.
pub struct Engine {
    buf: CodeBuffer,
    names: NameTable,
    pending: Vec<Pending>,
}

impl Engine {
    /// Create an engine backed by a buffer of at least `capacity` bytes.
    pub fn new(capacity: usize) -> Result<Self, DynError> {
        Ok(Self {
            buf: CodeBuffer::new(capacity)?,
            names: NameTable::new(),
            pending: Vec::new(),
        })
    }

    /// Number of registered names.
    pub fn name_count(&self) -> usize {
        self.names.len()
    }

    /// Number of still-unresolved forward references.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Absolute address a name resolves to, if defined.
    pub fn addr_of(&self, name: &str) -> Option<usize> {
        self.names.addr_of(name)
    }

    /// Register a foreign absolute address (for example a `dlsym` result) and
    /// backpatch any pending calls that were waiting for it.
    pub fn define_foreign(&mut self, name: &str, addr: usize) -> Result<(), DynError> {
        self.ensure_label_capacity(name)?;
        self.buf.make_writable()?;
        self.names.define_foreign(name, addr);
        self.resolve_pending()?;
        self.buf.make_executable()?;
        Ok(())
    }

    /// Assemble `ops`, append them to the live buffer, resolve every rel32 that
    /// can be resolved now, park the rest as pending, and leave the buffer
    /// executable.
    pub fn assemble(&mut self, ops: &[Op]) -> Result<(), DynError> {
        let enc = encode(ops)?;

        // Reject a batch whose new labels would exceed the cap before mutating.
        for (name, _) in &enc.labels {
            if self.names.addr_of(name).is_some() {
                return Err(DynError::Exec(format!("name `{name}` already defined")));
            }
        }
        if self.names.len() + enc.labels.len() > MAX_LABELS {
            return Err(DynError::Exec(format!(
                "label limit {MAX_LABELS} exceeded"
            )));
        }

        self.buf.make_writable()?;
        let land = self.buf.len();
        self.buf.append(&enc.bytes)?;

        for (name, local) in &enc.labels {
            self.names.define_emitted(name, &self.buf, land + local);
        }
        for fixup in &enc.fixups {
            let site = land + fixup.at;
            match self.names.addr_of(&fixup.target) {
                Some(target) => self.patch_rel32(site, target)?,
                None => self.park_pending(site, &fixup.target)?,
            }
        }
        // A label just defined in this batch may satisfy older pending calls.
        self.resolve_pending()?;

        self.buf.make_executable()?;
        Ok(())
    }

    /// Enter the code at `name` as `extern "C" fn() -> i64`.
    ///
    /// # Safety
    /// The bytes at `name` must be a valid function body for that signature and
    /// every name it calls must already be resolved (no pending fixup reachable
    /// from it); otherwise execution jumps through a placeholder displacement.
    /// The caller owns all ABI obligations, as with [`CodeBuffer::enter_i64`].
    pub unsafe fn enter_i64(&self, name: &str) -> Result<i64, DynError> {
        let addr = self
            .names
            .addr_of(name)
            .ok_or_else(|| DynError::Exec(format!("unknown entry name `{name}`")))?;
        let offset = addr - self.buf.base();
        // SAFETY: forwarded to the buffer's typed entry gate under the caller's
        // ABI contract; `offset` is within this buffer's emitted range.
        unsafe { self.buf.enter_i64(offset) }
    }

    fn ensure_label_capacity(&self, name: &str) -> Result<(), DynError> {
        if self.names.addr_of(name).is_some() {
            return Err(DynError::Exec(format!("name `{name}` already defined")));
        }
        if self.names.len() >= MAX_LABELS {
            return Err(DynError::Exec(format!("label limit {MAX_LABELS} exceeded")));
        }
        Ok(())
    }

    fn park_pending(&mut self, site: usize, target: &str) -> Result<(), DynError> {
        if self.pending.len() >= MAX_PENDING_FIXUPS {
            return Err(DynError::Exec(format!(
                "pending-fixup limit {MAX_PENDING_FIXUPS} exceeded"
            )));
        }
        self.pending.push(Pending {
            site,
            target: target.to_owned(),
        });
        Ok(())
    }

    /// Backpatch every pending site whose target is now defined. Assumes the
    /// buffer is already writable.
    fn resolve_pending(&mut self) -> Result<(), DynError> {
        let mut still = Vec::new();
        for p in std::mem::take(&mut self.pending) {
            match self.names.addr_of(&p.target) {
                Some(target) => self.patch_rel32(p.site, target)?,
                None => still.push(p),
            }
        }
        self.pending = still;
        Ok(())
    }

    /// Write the rel32 displacement for a call site, or fail loudly if it does
    /// not fit. Requires the buffer to be writable.
    fn patch_rel32(&mut self, site: usize, target_abs: usize) -> Result<(), DynError> {
        let site_abs = self.buf.base() + site;
        // rel32 is measured from the end of the 4-byte field.
        let rel = (target_abs as i64) - (site_abs as i64 + 4);
        let rel32 = i32::try_from(rel)
            .map_err(|_| DynError::Exec(format!("call displacement {rel} does not fit in rel32")))?;
        self.buf.patch(site, &rel32.to_le_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_reference_is_parked_then_patched() {
        let mut eng = Engine::new(256).unwrap();
        // Batch A calls `answer`, which is not defined yet.
        eng.assemble(&[
            Op::Label("main".into()),
            Op::Call("answer".into()),
            Op::Ret,
        ])
        .unwrap();
        assert_eq!(eng.pending_count(), 1);

        // Batch B defines `answer`; the parked site is backpatched.
        eng.assemble(&[Op::Label("answer".into()), Op::MovRaxImm(99), Op::Ret])
            .unwrap();
        assert_eq!(eng.pending_count(), 0);
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn rel32_overflow_fails_loudly() {
        let mut eng = Engine::new(64).unwrap();
        // A call site parked as pending, then resolved to address 0. On a
        // 64-bit host the buffer base is far above the i32 rel32 range, so the
        // displacement to 0 cannot fit and must fail loudly rather than emit a
        // truncated call.
        eng.assemble(&[Op::Label("m".into()), Op::Call("f".into()), Op::Ret])
            .unwrap();
        assert!(eng.define_foreign("f", 0).is_err());
    }
}
