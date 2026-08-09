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

// ---------------------------------------------------------------------------
// Real ELF-loaded images: a real `VirtualAlloc` mapping, not this crate's own
// heap `Vec<u8>` allocation. See `elf.rs`'s header for why a genuinely
// compiled static (non-PIE) ELF needs its segments landed at their REAL
// link-time vaddr rather than wherever Rust's global allocator happens to put
// a `Vec<u8>` -- non-PIE code contains real absolute-address immediates that
// only resolve correctly if "guest address == host address" holds EXACTLY,
// the same invariant this module's header already documents, reached here by
// asking Windows for that exact address instead of accepting whatever the
// allocator gives back.

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(addr: *mut u8, size: usize, alloc_type: u32, protect: u32) -> *mut u8;
    fn VirtualFree(addr: *mut u8, size: usize, free_type: u32) -> i32;
}

#[cfg(windows)]
const MEM_COMMIT: u32 = 0x0000_1000;
#[cfg(windows)]
const MEM_RESERVE: u32 = 0x0000_2000;
#[cfg(windows)]
const MEM_RELEASE: u32 = 0x0000_8000;
#[cfg(windows)]
const PAGE_READWRITE: u32 = 0x04;

/// A real `VirtualAlloc` mapping backing a `GuestImage.buf` built by
/// `elf::load_elf` -- owns the raw allocation and frees it (`VirtualFree`) on
/// drop. `GuestImage`'s own `Drop` impl (`lib.rs`) makes sure `buf`'s own
/// destructor never ALSO tries to free this same memory through the wrong
/// allocator; see that impl's comment for the full mechanism.
pub struct MappedRegion {
    ptr: *mut u8,
    len: usize,
}

impl MappedRegion {
    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            VirtualFree(self.ptr, 0, MEM_RELEASE);
        }
    }
}

/// Reserve+commit `len` real, zero-initialized, read-write bytes at EXACTLY
/// host address `addr` (the caller page-aligns it first) -- Windows' closest
/// equivalent to Linux `mmap(addr, len, ..., MAP_FIXED, ...)`. Always
/// `PAGE_READWRITE`, never `PAGE_EXECUTE_*` -- this crate never requests
/// executable host memory for guest bytes (see the crate README), so a
/// `PT_LOAD` segment flagged `PF_X` in the ELF is mapped exactly the same as
/// any other; guest bytes are only ever fetched as interpreter DATA, never
/// executed by the host CPU.
///
/// Returns `None` -- an HONEST failure, not a silent rebase to a different
/// address -- if Windows can't hand back `addr` exactly: either
/// `VirtualAlloc` itself fails outright (e.g. the range is already in use by
/// this process), or it succeeds but at a different address because `addr`
/// wasn't already a multiple of the 64KiB allocation granularity Windows
/// silently rounds down to; that second case is checked explicitly below and
/// the mismatched region is freed immediately rather than kept and used as a
/// base that would break every absolute-address immediate a non-PIE guest
/// binary contains.
#[cfg(windows)]
pub fn alloc_fixed(addr: u64, len: usize) -> Option<MappedRegion> {
    let ptr = unsafe { VirtualAlloc(addr as *mut u8, len, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if ptr.is_null() {
        return None;
    }
    if ptr as u64 != addr {
        unsafe { VirtualFree(ptr, 0, MEM_RELEASE) };
        return None;
    }
    Some(MappedRegion { ptr, len })
}

/// Non-Windows stub -- this crate's whole syscall-translation layer targets
/// real Win32 calls (see the crate README), so there is no real mapping to
/// attempt here; always an honest `None`, never a fake success.
#[cfg(not(windows))]
pub fn alloc_fixed(_addr: u64, _len: usize) -> Option<MappedRegion> {
    None
}
