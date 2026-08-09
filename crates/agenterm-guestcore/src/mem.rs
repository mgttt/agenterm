//! Real host memory access for the guest's single flat address space.
//!
//! This crate's whole addressing model is "guest address == host address" --
//! the same invariant `ape-vm`'s `MAP_FIXED` direct mapping establishes for a
//! real ELF loader, reached here the cheap way (Phase 1 scope, see the design
//! doc §0/§1): the guest image is one `Vec<u8>` allocated once and never
//! resized afterward, so `buf.as_ptr()` is a stable real host pointer for the
//! rest of the run. Every register that the guest computes as a "pointer"
//! (via `lea`, `mov`, arithmetic, or a syscall return value like `mmap`'s)
//! is, by construction, ALSO a valid host pointer -- there is no separate
//! guest/host translation step at dereference time. This is deliberate and
//! matches `ape-vm`'s own non-sandboxed model: Phase 1 proves the mechanism
//! (real machine code, real syscalls translated to real Win32 calls), not a
//! memory-safety sandbox around untrusted guest code. A guest program that
//! computes a bad address really dereferences a bad host pointer here, same
//! as it would on real hardware (a hardware fault, not a software lie).
//!
//! The one bug this crate exists to NOT regress (found building the
//! reference `guest_interp.rs` prototype): a RIP-relative `lea` that forgets
//! to add the buffer's real base address computes a small guest-relative
//! offset instead of a real pointer; dereferencing that "pointer" segfaults.
//! `decode.rs`'s `MemAddr::RipRel` resolution and every call site below are
//! the fix -- `base` is added exactly once, at the point a `RipRel` operand
//! is turned into an `Abs` address, never skipped.

/// # Safety
/// `addr` must be a valid, readable host pointer for the access width (a
/// guest-computed pointer into the flat image, the stack region within it,
/// or a `VirtualAlloc` region returned by a translated `mmap` syscall all
/// qualify -- see this module's header). This crate only ever calls these
/// helpers with addresses a guest register produced via `lea`/arithmetic
/// from such a base, or a syscall return value of the same kind.
#[inline]
pub unsafe fn read_u8(addr: u64) -> u8 {
    unsafe { *(addr as *const u8) }
}

/// # Safety
/// `addr` must be a valid, writable host pointer -- see [`read_u8`]'s
/// `# Safety` section, same requirement.
#[inline]
pub unsafe fn write_u8(addr: u64, val: u8) {
    unsafe { *(addr as *mut u8) = val }
}

/// # Safety
/// `addr` must be a valid, readable host pointer for 4 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn read_u32(addr: u64) -> u32 {
    unsafe { (addr as *const u8).cast::<u32>().read_unaligned() }
}

/// # Safety
/// `addr` must be a valid, writable host pointer for 4 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn write_u32(addr: u64, val: u32) {
    unsafe { (addr as *mut u8).cast::<u32>().write_unaligned(val) }
}

/// # Safety
/// `addr` must be a valid, readable host pointer for 2 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn read_u16(addr: u64) -> u16 {
    unsafe { (addr as *const u8).cast::<u16>().read_unaligned() }
}

/// # Safety
/// `addr` must be a valid, writable host pointer for 2 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn write_u16(addr: u64, val: u16) {
    unsafe { (addr as *mut u8).cast::<u16>().write_unaligned(val) }
}

/// # Safety
/// `addr` must be a valid, readable host pointer for 8 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn read_u64(addr: u64) -> u64 {
    unsafe { (addr as *const u8).cast::<u64>().read_unaligned() }
}

/// # Safety
/// `addr` must be a valid, writable host pointer for 8 bytes -- see
/// [`read_u8`]'s `# Safety` section.
#[inline]
pub unsafe fn write_u64(addr: u64, val: u64) {
    unsafe { (addr as *mut u8).cast::<u64>().write_unaligned(val) }
}
