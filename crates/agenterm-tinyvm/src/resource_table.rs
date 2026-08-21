//! Bounded host-resource ownership behind guest-visible `i32` handles.
//!
//! Native modules can keep platform objects in this table and pass only the
//! returned 32-bit token through a standard Wasm function import. Tokens carry
//! a slot generation, so closing and reusing a slot does not make an old token
//! name the new object.

use alloc::vec::Vec;

/// A non-zero, generation-checked token suitable for an `i32` Wasm ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GuestResourceHandle(u32);

impl GuestResourceHandle {
    /// Reconstruct a handle received through an embedding ABI.
    ///
    /// Zero and encodings with a zero slot or generation are never valid.
    pub const fn from_raw(raw: u32) -> Option<Self> {
        if raw & 0xffff == 0 || raw >> 16 == 0 {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// Reconstruct a token received as a Wasm `i32`, preserving its bits.
    pub const fn from_i32(raw: i32) -> Option<Self> {
        Self::from_raw(raw as u32)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn as_i32(self) -> i32 {
        self.0 as i32
    }

    fn parts(self) -> (usize, u16) {
        let slot = ((self.0 & 0xffff) - 1) as usize;
        let generation = (self.0 >> 16) as u16;
        (slot, generation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceTableError {
    Full,
    AllocationFailed,
    StaleHandle,
}

struct Slot<T> {
    /// Zero permanently retires a slot after its generation space is spent.
    generation: u16,
    value: Option<T>,
}

/// One native module's bounded owner for host resources exposed to a guest.
///
/// A table supports at most 65,535 slots. Removing a value invalidates its
/// handle before the slot can be reused. A slot is permanently retired rather
/// than allowing its generation to wrap and alias a very old handle.
pub struct HostResourceTable<T> {
    slots: Vec<Slot<T>>,
    len: u16,
    max_resources: u16,
}

impl<T> HostResourceTable<T> {
    pub const fn new(max_resources: u16) -> Self {
        Self {
            slots: Vec::new(),
            len: 0,
            max_resources,
        }
    }

    pub const fn len(&self) -> u16 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn max_resources(&self) -> u16 {
        self.max_resources
    }

    /// Whether an insertion can be published without exceeding the table's
    /// logical resource limit. The eventual vector growth can still fail.
    pub fn has_capacity(&self) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.generation != 0 && slot.value.is_none())
            || self.slots.len() < self.max_resources as usize
    }

    /// Takes ownership and returns a guest token, or drops the supplied value
    /// while returning a typed failure.
    pub fn insert(&mut self, value: T) -> Result<GuestResourceHandle, ResourceTableError> {
        if let Some((index, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.generation != 0 && slot.value.is_none())
        {
            slot.value = Some(value);
            self.len += 1;
            return Ok(make_handle(index, slot.generation));
        }

        if self.slots.len() >= self.max_resources as usize {
            return Err(ResourceTableError::Full);
        }
        self.slots
            .try_reserve_exact(1)
            .map_err(|_| ResourceTableError::AllocationFailed)?;
        let index = self.slots.len();
        self.slots.push(Slot {
            generation: 1,
            value: Some(value),
        });
        self.len += 1;
        Ok(make_handle(index, 1))
    }

    pub fn get(&self, handle: GuestResourceHandle) -> Result<&T, ResourceTableError> {
        let (index, generation) = handle.parts();
        let slot = self
            .slots
            .get(index)
            .filter(|slot| slot.generation == generation)
            .ok_or(ResourceTableError::StaleHandle)?;
        slot.value.as_ref().ok_or(ResourceTableError::StaleHandle)
    }

    pub fn get_mut(&mut self, handle: GuestResourceHandle) -> Result<&mut T, ResourceTableError> {
        let (index, generation) = handle.parts();
        let slot = self
            .slots
            .get_mut(index)
            .filter(|slot| slot.generation == generation)
            .ok_or(ResourceTableError::StaleHandle)?;
        slot.value.as_mut().ok_or(ResourceTableError::StaleHandle)
    }

    /// Removes and returns the owned resource. The handle is invalid before
    /// another value can occupy the slot.
    pub fn remove(&mut self, handle: GuestResourceHandle) -> Result<T, ResourceTableError> {
        let (index, generation) = handle.parts();
        let slot = self
            .slots
            .get_mut(index)
            .filter(|slot| slot.generation == generation)
            .ok_or(ResourceTableError::StaleHandle)?;
        let value = slot.value.take().ok_or(ResourceTableError::StaleHandle)?;
        self.len -= 1;
        advance_generation(slot);
        Ok(value)
    }

    /// Drops every live resource and invalidates every issued handle.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            if slot.value.take().is_some() {
                advance_generation(slot);
            }
        }
        self.len = 0;
    }
}

fn make_handle(index: usize, generation: u16) -> GuestResourceHandle {
    debug_assert!(index < u16::MAX as usize);
    debug_assert_ne!(generation, 0);
    GuestResourceHandle((u32::from(generation) << 16) | (index as u32 + 1))
}

fn advance_generation<T>(slot: &mut Slot<T>) {
    slot.generation = if slot.generation == u16::MAX {
        0
    } else {
        slot.generation + 1
    };
}
