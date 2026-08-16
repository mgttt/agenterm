//! Execution base: the in-process live code buffer.
//!
//! This is dyn's first cut at *being the machine* rather than interpreting a
//! list language. Three pieces, each able to grow while the process runs:
//!
//! - [`CodeBuffer`] — an mmap'd page holding host-ISA bytes, with a **W^X**
//!   discipline from the first day: it is either writable or executable, never
//!   both. Bytes are appended in the writable state; execution is only possible
//!   after an explicit flip to the executable state.
//! - [`NameTable`] — names resolving to an absolute address: either an offset
//!   into a buffer (a symbol the engine emitted) or a raw address handed in
//!   from `dlsym` (an outward call gate).
//! - the typed *enter* gate ([`CodeBuffer::enter_i64`]) — the caller declares a
//!   C signature and jumps into the buffer.
//!
//! Safety split (this crate's identity after the exec-base cut): **staging
//! bytes is safe**; appending, flipping, and reading addresses cannot execute
//! anything. **Jumping in is `unsafe`**: the caller owns that the bytes at the
//! target offset are a valid function body for the declared ABI.
//!
//! Scope of this cut: Linux/Unix `mmap` + `mprotect` toggling. Byte encodings
//! are hand-written here (helpers below); this file does **not** contain an
//! assembler or a general relocation/patch table — those are later cuts. macOS
//! hardened-runtime JIT (`MAP_JIT` + `pthread_jit_write_protect_np`) and any
//! Windows execution path are deliberately out of scope here.

use std::collections::HashMap;

use crate::error::DynError;

#[cfg(target_os = "linux")]
const MAP_ANON: i32 = libc::MAP_ANONYMOUS;
#[cfg(not(target_os = "linux"))]
const MAP_ANON: i32 = libc::MAP_ANON;

fn page_size() -> usize {
    // SAFETY: sysconf with a fixed, side-effect-free query name.
    let raw = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if raw <= 0 { 4096 } else { raw as usize }
}

/// Current protection state of a [`CodeBuffer`]. There is intentionally no
/// third `ReadWriteExec` state: W^X means the two are mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferState {
    /// Writable, not executable. Bytes may be appended.
    Writable,
    /// Executable, not writable. Entries may be called.
    Executable,
}

/// An mmap'd host-ISA code buffer with a W^X protection discipline.
pub struct CodeBuffer {
    ptr: *mut u8,
    mapped: usize,
    filled: usize,
    state: BufferState,
}

impl CodeBuffer {
    /// Map a fresh buffer with room for at least `capacity` bytes.
    ///
    /// The buffer starts [`BufferState::Writable`] so bytes can be staged; it
    /// is never mapped writable-and-executable at once.
    pub fn new(capacity: usize) -> Result<Self, DynError> {
        let cap = capacity.max(1);
        let ps = page_size();
        let mapped = cap.div_ceil(ps) * ps;
        // SAFETY: anonymous private mapping, fixed args; the returned pointer is
        // checked against MAP_FAILED before any use.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                mapped,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            let err = std::io::Error::last_os_error();
            return Err(DynError::Exec(format!("mmap failed: {err}")));
        }
        Ok(Self {
            ptr: ptr as *mut u8,
            mapped,
            filled: 0,
            state: BufferState::Writable,
        })
    }

    /// Current W^X state.
    pub fn state(&self) -> BufferState {
        self.state
    }

    /// Bytes appended so far.
    pub fn len(&self) -> usize {
        self.filled
    }

    /// Whether nothing has been appended yet.
    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Append host-ISA `bytes`, returning the offset at which they start.
    ///
    /// Requires the buffer to be [`BufferState::Writable`]; appending to an
    /// executable buffer is a W^X violation and is rejected loudly rather than
    /// silently corrupting live code. A full buffer is likewise rejected — this
    /// cut does not grow or remap.
    pub fn append(&mut self, bytes: &[u8]) -> Result<usize, DynError> {
        if self.state != BufferState::Writable {
            return Err(DynError::Exec(
                "append requires a writable buffer (W^X); call make_writable first".into(),
            ));
        }
        let offset = self.filled;
        let end = offset
            .checked_add(bytes.len())
            .ok_or_else(|| DynError::Exec("append length overflow".into()))?;
        if end > self.mapped {
            return Err(DynError::Exec(format!(
                "append of {} bytes exceeds buffer capacity {} (filled {})",
                bytes.len(),
                self.mapped,
                self.filled
            )));
        }
        // SAFETY: `offset..end` is within the mapped, currently-writable region.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.ptr.add(offset), bytes.len());
        }
        self.filled = end;
        Ok(offset)
    }

    /// Flip to [`BufferState::Executable`] (read+exec, not writable).
    pub fn make_executable(&mut self) -> Result<(), DynError> {
        self.protect(libc::PROT_READ | libc::PROT_EXEC)?;
        self.state = BufferState::Executable;
        Ok(())
    }

    /// Flip back to [`BufferState::Writable`] (read+write, not executable).
    pub fn make_writable(&mut self) -> Result<(), DynError> {
        self.protect(libc::PROT_READ | libc::PROT_WRITE)?;
        self.state = BufferState::Writable;
        Ok(())
    }

    fn protect(&self, prot: i32) -> Result<(), DynError> {
        // SAFETY: mprotect over exactly this mapping's page range.
        let rc = unsafe { libc::mprotect(self.ptr as *mut libc::c_void, self.mapped, prot) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            return Err(DynError::Exec(format!("mprotect failed: {err}")));
        }
        Ok(())
    }

    /// Absolute address of `offset` within the buffer.
    pub fn addr_of(&self, offset: usize) -> usize {
        self.ptr as usize + offset
    }

    /// Base address of the buffer.
    pub fn base(&self) -> usize {
        self.ptr as usize
    }

    /// Jump into the buffer at `offset` as `extern "C" fn() -> i64` and return
    /// its result.
    ///
    /// # Safety
    /// The bytes at `offset` must be a complete, valid function body for the
    /// SysV/AAPCS C ABI of `extern "C" fn() -> i64` on the host, ending in a
    /// return (or a tail jump whose target returns). The buffer must be
    /// [`BufferState::Executable`]; the caller owns every register/stack/ABI
    /// obligation of the emitted code, exactly as with `dlcall`.
    pub unsafe fn enter_i64(&self, offset: usize) -> Result<i64, DynError> {
        if self.state != BufferState::Executable {
            return Err(DynError::Exec(
                "enter requires an executable buffer (W^X); call make_executable first".into(),
            ));
        }
        if offset >= self.filled {
            return Err(DynError::Exec(format!(
                "enter offset {offset} is outside filled range 0..{}",
                self.filled
            )));
        }
        let addr = self.addr_of(offset);
        // SAFETY: caller-owned per the doc contract; addr is within the mapped,
        // executable region and points at emitted host-ISA bytes.
        let f: extern "C" fn() -> i64 = unsafe { std::mem::transmute(addr) };
        Ok(f())
    }
}

impl Drop for CodeBuffer {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly the region returned by mmap.
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, self.mapped);
        }
    }
}

/// A name resolving to an absolute address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameEntry {
    /// A symbol emitted into a code buffer, recorded as its absolute address.
    Emitted(usize),
    /// A raw address handed in from outside the engine (for example a `dlsym`
    /// result): the outward call gate.
    Foreign(usize),
}

impl NameEntry {
    /// The absolute address this name resolves to.
    pub fn addr(self) -> usize {
        match self {
            Self::Emitted(a) | Self::Foreign(a) => a,
        }
    }
}

/// Names to addresses: the fourth piece of the engine's live state.
///
/// This is the minimal call-gate/name registry for the exec-base cut. It is
/// deliberately not the general patch/relocation table — forward references and
/// fixups are a later cut.
#[derive(Debug, Default)]
pub struct NameTable {
    entries: HashMap<String, NameEntry>,
}

impl NameTable {
    /// A fresh, empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `name` as an offset into `buffer` (an emitted symbol).
    pub fn define_emitted(&mut self, name: &str, buffer: &CodeBuffer, offset: usize) {
        self.entries
            .insert(name.to_owned(), NameEntry::Emitted(buffer.addr_of(offset)));
    }

    /// Record `name` as a foreign absolute address (for example from `dlsym`).
    pub fn define_foreign(&mut self, name: &str, addr: usize) {
        self.entries
            .insert(name.to_owned(), NameEntry::Foreign(addr));
    }

    /// Resolve `name` to its entry, if present.
    pub fn resolve(&self, name: &str) -> Option<NameEntry> {
        self.entries.get(name).copied()
    }

    /// Resolve `name` directly to an absolute address, if present.
    pub fn addr_of(&self, name: &str) -> Option<usize> {
        self.resolve(name).map(NameEntry::addr)
    }

    /// Number of recorded names.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no names are recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// -- Hand-written host-ISA encodings ------------------------------------------
//
// These are single hand-emitted snippets for the exec-base acceptance, not a
// general encoder. x86_64 encodings execute on-host; the aarch64 encoding is a
// golden byte sequence for cross-ISA parity, not executed on an x86_64 host.

/// x86_64: `mov rax, imm64` then `ret` — returns `imm` as an `i64`.
///
/// `48 B8 <imm64 LE>` (movabs rax, imm64) + `C3` (ret).
pub fn x86_64_mov_rax_ret(imm: i64) -> Vec<u8> {
    let mut code = Vec::with_capacity(11);
    code.push(0x48);
    code.push(0xB8);
    code.extend_from_slice(&imm.to_le_bytes());
    code.push(0xC3);
    code
}

/// x86_64: a stack-aligned `call` into `target_addr`, returning its result.
///
/// On entry `rsp % 16 == 8` (the CPU pushed our return address onto a
/// 16-aligned stack). SysV requires `rsp % 16 == 0` at the point of a `call`,
/// so we `sub rsp, 8` first, then `add rsp, 8` and `ret`.
///
/// `48 83 EC 08` (sub rsp,8) · `48 B8 <imm64>` (movabs rax,target) ·
/// `FF D0` (call rax) · `48 83 C4 08` (add rsp,8) · `C3` (ret).
pub fn x86_64_call_thunk(target_addr: usize) -> Vec<u8> {
    let mut code = Vec::with_capacity(20);
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x08]); // sub rsp, 8
    code.push(0x48);
    code.push(0xB8); // movabs rax, imm64
    code.extend_from_slice(&(target_addr as u64).to_le_bytes());
    code.extend_from_slice(&[0xFF, 0xD0]); // call rax
    code.extend_from_slice(&[0x48, 0x83, 0xC4, 0x08]); // add rsp, 8
    code.push(0xC3); // ret
    code
}

/// aarch64: `movz x0, #imm16` then `ret` — the arm64 counterpart of
/// [`x86_64_mov_rax_ret`] for `0 <= imm <= 0xFFFF`.
///
/// `MOVZ x0, #imm` = `0xD2800000 | (imm << 5)` · `RET` = `0xD65F03C0`, both
/// little-endian. Golden bytes for cross-ISA parity; not executed on x86_64.
pub fn aarch64_mov_x0_ret(imm: u16) -> Vec<u8> {
    let movz: u32 = 0xD280_0000 | ((imm as u32) << 5);
    let ret: u32 = 0xD65F_03C0;
    let mut code = Vec::with_capacity(8);
    code.extend_from_slice(&movz.to_le_bytes());
    code.extend_from_slice(&ret.to_le_bytes());
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wx_discipline_starts_writable_and_toggles() {
        let mut buf = CodeBuffer::new(64).expect("map");
        assert_eq!(buf.state(), BufferState::Writable);
        buf.append(&[0x90]).expect("append nop");
        buf.make_executable().expect("to exec");
        assert_eq!(buf.state(), BufferState::Executable);
        // Appending while executable is a W^X violation and must be rejected.
        assert!(buf.append(&[0x90]).is_err());
        buf.make_writable().expect("back to writable");
        assert_eq!(buf.state(), BufferState::Writable);
        buf.append(&[0x90]).expect("append after flip back");
    }

    #[test]
    fn enter_requires_executable_state() {
        let mut buf = CodeBuffer::new(64).expect("map");
        let off = buf.append(&x86_64_mov_rax_ret(1)).expect("append");
        // Still writable: entering must be refused.
        // SAFETY: call is refused before any transmute/jump because the buffer
        // is not executable.
        let err = unsafe { buf.enter_i64(off) };
        assert!(err.is_err());
    }

    #[test]
    fn append_rejects_overflow() {
        let mut buf = CodeBuffer::new(8).expect("map");
        // mapping is page-rounded, so pick a length certain to exceed it.
        let huge = vec![0u8; buf.mapped + 1];
        assert!(buf.append(&huge).is_err());
    }

    #[test]
    fn aarch64_golden_mov_x0_42_ret() {
        // MOVZ x0,#42 = 0xD2800540 ; RET = 0xD65F03C0, little-endian.
        assert_eq!(
            aarch64_mov_x0_ret(42),
            vec![0x40, 0x05, 0x80, 0xD2, 0xC0, 0x03, 0x5F, 0xD6]
        );
    }

    #[test]
    fn name_table_records_emitted_and_foreign() {
        let mut buf = CodeBuffer::new(64).expect("map");
        let off = buf.append(&[0xC3]).expect("append ret");
        let mut names = NameTable::new();
        names.define_emitted("local_ret", &buf, off);
        names.define_foreign("some_symbol", 0xDEAD_BEEF);
        assert_eq!(names.addr_of("local_ret"), Some(buf.addr_of(off)));
        assert_eq!(
            names.resolve("some_symbol"),
            Some(NameEntry::Foreign(0xDEAD_BEEF))
        );
        assert_eq!(names.addr_of("missing"), None);
        assert_eq!(names.len(), 2);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn enter_returns_movabs_immediate() {
        let mut buf = CodeBuffer::new(64).expect("map");
        let off = buf.append(&x86_64_mov_rax_ret(42)).expect("append");
        buf.make_executable().expect("to exec");
        // SAFETY: the bytes at `off` are a valid `extern "C" fn() -> i64`.
        let got = unsafe { buf.enter_i64(off) }.expect("enter");
        assert_eq!(got, 42);
    }
}
