//! Real ELF64 loading: parse a genuinely compiled static x86_64 Linux ELF
//! executable (`ET_EXEC`, non-PIE) and populate a `GuestImage` + initial
//! `Cpu` state (entry point, real memory image, stack with argv/envp/auxv)
//! so the EXISTING interpreter loop (`lib.rs::run_guest`, its decode/ABI/
//! intent-map pipeline entirely unchanged) can execute it. This module owns
//! exactly two things: (1) parsing the ELF64 header + program header table
//! (pure data, no I/O side effects, no allocation of guest memory), and
//! (2) turning `PT_LOAD` segments into a real host memory image via
//! `mem::alloc_fixed` -- the same `MAP_FIXED`-style direct mapping
//! `ape-vm`'s README documents for static non-PIE ELF (`guest vaddr == host
//! address`, reached here with `VirtualAlloc`'s explicit-address form since
//! Windows has no literal `MAP_FIXED`; see `mem.rs::alloc_fixed`'s own
//! header for exactly how that mapping is verified, not just requested).
//!
//! ## Why one `VirtualAlloc` call for the whole segment span, not one per
//! `PT_LOAD` (unlike `ape-vm`'s literal per-segment `MAP_FIXED`)
//!
//! This crate's interpreter (`decode_x86_64::step`) fetches guest bytes by
//! indexing a single contiguous `&[u8]` with `cpu.rip` as a plain offset
//! (see `cpu.rs`/`lib.rs::GuestImage`) -- it was not built to fetch across
//! several disjoint mappings. Real linkers pack `PT_LOAD` segments
//! contiguously in *virtual address* space already (only page-alignment
//! padding between them, never large gaps), so reserving one span from the
//! lowest to the highest segment's vaddr and copying each segment's bytes
//! to its correct offset within that single span is equivalent to mapping
//! each segment separately -- and it is the exact technique `ape-vm`'s own
//! README documents for `ET_DYN` (`load_base = m - min_vaddr`, one-shot
//! `mmap`), reused here for `ET_EXEC` too because it is what this
//! interpreter's fetch loop requires. For `ET_EXEC` the span is additionally
//! pinned to land at EXACTLY `min_vaddr` (not wherever the OS would have
//! chosen for a relocatable one-shot mapping), preserving `ET_EXEC`'s
//! defining property: absolute address immediates baked in at link time
//! resolve correctly with no relocation step.
//!
//! The whole span (code, rodata, data, bss, and the stack appended after
//! it) is mapped `PAGE_READWRITE`, never `PAGE_EXECUTE_*`, EVEN for the
//! `PF_X`-flagged text segment -- consistent with this crate's own
//! discipline (see the crate README) of never requesting executable host
//! memory for guest bytes: they are only ever fetched as interpreter DATA,
//! never executed by the host CPU.
//!
//! ## Scope (read this before assuming more is supported)
//!
//! - **`ET_EXEC` (static, non-PIE) only.** `ET_DYN` (PIE) and any ELF with a
//!   `PT_INTERP` segment (dynamic linking) are honest, explicit
//!   `ElfLoadError`s, not attempted -- see `plan/design-dynacore-emulated-
//!   guest-core.md`'s own staging and this round's dispatch instructions
//!   ("PIE/ET_DYN is a stretch goal, don't block on it").
//! - **No relocations are processed at all**, not even a stub. Not needed
//!   for `ET_EXEC` (its addresses are already absolute at link time, by
//!   construction) -- worth stating plainly anyway: this loader does not
//!   walk `.rela.dyn`/`.rela.plt` under any circumstance.
//! - **Minimal stack ABI, not full System V.** `argc`/`argv`/an EMPTY
//!   `envp` (just its `NULL` terminator) are set up; the auxv vector
//!   present on the stack is JUST an `AT_NULL` terminator pair -- no
//!   `AT_PHDR`/`AT_PHENT`/`AT_PHNUM`/`AT_ENTRY`/`AT_BASE`/`AT_RANDOM`/
//!   `AT_PAGESZ`/etc. A real program whose startup calls `getauxval` for
//!   any of those (glibc's TLS/stack-canary setup does, at minimum; so does
//!   a `std`-linked Rust binary's runtime init even against musl -- it also
//!   needs `arch_prctl`, `sigaltstack`, `rt_sigaction`, `set_tid_address`,
//!   `rseq`, none of which `abi_linux_x86_64.rs`/`intent_map.rs` translate)
//!   will not run correctly through this loader. This is exactly why this
//!   round's own verification target is a `#![no_std] #![no_main]`
//!   raw-`_start` program that never touches argv/envp/auxv or any of those
//!   syscalls at all, not a `std`-using binary -- see the crate README's
//!   ELF section for the full reasoning.
//! - **BSS is zeroed explicitly by this loader** (belt-and-suspenders on
//!   top of the fact that a fresh `VirtualAlloc(MEM_COMMIT)` already hands
//!   back zeroed pages -- documented Win32 behavior, not an unstated
//!   assumption this loader silently depends on alone).

use crate::mem::{self, MappedRegion};
use crate::GuestImage;

pub const ELFCLASS64: u8 = 2;
pub const ELFDATA2LSB: u8 = 1;
pub const EM_X86_64: u16 = 62;
pub const ET_EXEC: u16 = 2;
#[allow(dead_code)] // named for ET_DYN-rejection-message completeness; not a supported path this round
pub const ET_DYN: u16 = 3;
pub const PT_LOAD: u32 = 1;
pub const PT_INTERP: u32 = 3;

const PAGE_SIZE: u64 = 0x1000;
const ELF_HEADER_LEN: usize = 64;
const PHDR_MIN_LEN: usize = 56;

/// Every way this loader refuses to guess -- same discipline as
/// `GuestFault` (see `fault.rs`'s header): an ELF this loader cannot
/// honestly place in real memory is always a clear `ElfLoadError`, never a
/// silently wrong load.
#[derive(Debug)]
pub enum ElfLoadError {
    /// The file is shorter than a fixed-size structure this loader needs to
    /// read (`need` bytes, starting at whatever offset that structure
    /// begins at) -- covers both "shorter than one ELF64 header" and
    /// "program header table runs past EOF".
    TooShort { need: usize, got: usize },
    /// First 4 bytes are not `\x7fELF`.
    BadMagic,
    /// `e_ident[EI_CLASS]` is not `ELFCLASS64` (2) -- this loader only reads
    /// the 64-bit ELF layout.
    UnsupportedClass { class: u8 },
    /// `e_ident[EI_DATA]` is not `ELFDATA2LSB` (1) -- x86_64 is always
    /// little-endian; a big-endian-flagged file is not a real x86_64 ELF.
    UnsupportedEndian { data: u8 },
    /// `e_machine` is not `EM_X86_64` (62) -- same-ISA-only scope (crate
    /// README), matching `decode_x86_64.rs`.
    UnsupportedMachine { machine: u16 },
    /// `e_type` is not `ET_EXEC` -- `ET_DYN` (PIE) is this round's stretch
    /// goal (not attempted), and `ET_REL`/`ET_CORE` are not executable
    /// images at all.
    UnsupportedType { e_type: u16 },
    /// A `PT_INTERP` segment is present -- this ELF wants a dynamic linker
    /// (`/lib64/ld-linux-x86-64.so.2` or similar), which this loader does
    /// not implement at all (no `.dynamic`/relocation/symbol-resolution
    /// support, see this module's header).
    HasInterp,
    /// No `PT_LOAD` segment at all -- nothing to place in memory, not a
    /// real executable image.
    NoLoadSegments,
    /// A `PT_LOAD` segment's `[p_offset, p_offset+p_filesz)` file range
    /// runs past the end of the actual file bytes handed to `load_elf`.
    SegmentOutOfFile { p_offset: u64, p_filesz: u64, file_len: usize },
    /// `mem::alloc_fixed` could not place the image at the exact host
    /// address this `ET_EXEC` binary's real link-time vaddr requires (see
    /// `mem.rs::alloc_fixed`'s header for the two ways that can honestly
    /// happen) -- a real, reportable limitation, not something this loader
    /// silently works around by rebasing (which would break every
    /// absolute-address immediate the guest code contains).
    FixedMapFailed { addr: u64, len: usize },
}

impl std::fmt::Display for ElfLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElfLoadError::TooShort { need, got } => write!(f, "elf: file too short (needed at least {need} bytes for a structure this loader reads, file is {got})"),
            ElfLoadError::BadMagic => write!(f, "elf: bad magic (not \\x7fELF)"),
            ElfLoadError::UnsupportedClass { class } => write!(f, "elf: unsupported EI_CLASS {class} (only ELFCLASS64=2 supported)"),
            ElfLoadError::UnsupportedEndian { data } => write!(f, "elf: unsupported EI_DATA {data} (only ELFDATA2LSB=1 / little-endian supported)"),
            ElfLoadError::UnsupportedMachine { machine } => write!(f, "elf: unsupported e_machine {machine} (only EM_X86_64=62 supported -- same-ISA-only scope)"),
            ElfLoadError::UnsupportedType { e_type } => write!(f, "elf: unsupported e_type {e_type} (only ET_EXEC=2 static-non-PIE supported this round; ET_DYN=3 PIE is a stretch goal not attempted)"),
            ElfLoadError::HasInterp => write!(f, "elf: PT_INTERP segment present -- this ELF wants a dynamic linker, which this loader does not implement"),
            ElfLoadError::NoLoadSegments => write!(f, "elf: no PT_LOAD segments -- nothing to place in memory"),
            ElfLoadError::SegmentOutOfFile { p_offset, p_filesz, file_len } => {
                write!(f, "elf: PT_LOAD segment file range [{p_offset:#x}, {:#x}) runs past end of file ({file_len} bytes)", p_offset + p_filesz)
            }
            ElfLoadError::FixedMapFailed { addr, len } => {
                write!(f, "elf: could not map {len} bytes at exact host address {addr:#x} (VirtualAlloc failed or landed elsewhere -- see mem.rs::alloc_fixed)")
            }
        }
    }
}

impl std::error::Error for ElfLoadError {}

/// One `PT_LOAD` program header entry -- only the fields this loader
/// actually uses (`p_paddr`/`p_align` are read by real loaders too, but
/// nothing here needs them: `p_align` is folded into the page-alignment
/// this loader always applies, `p_paddr` has no meaning for a Linux
/// userspace ELF).
#[derive(Clone, Copy, Debug)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
}

/// The parsed-out subset of an ELF64 file this loader needs: entry point,
/// every `PT_LOAD` segment, and whether a `PT_INTERP` segment was present.
/// Pure data -- `parse_elf64` never allocates guest memory or touches
/// Windows at all, that's `load_elf`'s job below.
#[derive(Debug)]
pub struct ElfFile {
    pub e_type: u16,
    pub e_entry: u64,
    pub segments: Vec<ProgramHeader>,
    pub has_interp: bool,
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().expect("4 bytes"))
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().expect("8 bytes"))
}

/// Parse an ELF64 header + program header table. Field offsets are the
/// standard `Elf64_Ehdr`/`Elf64_Phdr` layout (public CPU/ABI-manual
/// knowledge, not copied from any particular implementation's source):
/// `e_type`@16, `e_machine`@18, `e_entry`@24, `e_phoff`@32, `e_phentsize`@54,
/// `e_phnum`@56 in a 64-byte `Elf64_Ehdr`; `p_type`@0, `p_flags`@4,
/// `p_offset`@8, `p_vaddr`@16, `p_filesz`@32, `p_memsz`@40 in each
/// (>=56-byte) `Elf64_Phdr` entry.
pub fn parse_elf64(bytes: &[u8]) -> Result<ElfFile, ElfLoadError> {
    if bytes.len() < ELF_HEADER_LEN {
        return Err(ElfLoadError::TooShort { need: ELF_HEADER_LEN, got: bytes.len() });
    }
    if bytes[0..4] != [0x7f, b'E', b'L', b'F'] {
        return Err(ElfLoadError::BadMagic);
    }
    let class = bytes[4];
    if class != ELFCLASS64 {
        return Err(ElfLoadError::UnsupportedClass { class });
    }
    let data = bytes[5];
    if data != ELFDATA2LSB {
        return Err(ElfLoadError::UnsupportedEndian { data });
    }

    let e_type = u16_at(bytes, 16);
    let e_machine = u16_at(bytes, 18);
    if e_machine != EM_X86_64 {
        return Err(ElfLoadError::UnsupportedMachine { machine: e_machine });
    }
    let e_entry = u64_at(bytes, 24);
    let e_phoff = u64_at(bytes, 32) as usize;
    let e_phentsize = u16_at(bytes, 54) as usize;
    let e_phnum = u16_at(bytes, 56) as usize;

    if e_phnum > 0 && e_phentsize < PHDR_MIN_LEN {
        return Err(ElfLoadError::TooShort { need: PHDR_MIN_LEN, got: e_phentsize });
    }
    let ph_table_end = e_phoff + e_phentsize * e_phnum;
    if ph_table_end > bytes.len() {
        return Err(ElfLoadError::TooShort { need: ph_table_end, got: bytes.len() });
    }

    let mut segments = Vec::new();
    let mut has_interp = false;
    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        let p_type = u32_at(bytes, off);
        if p_type == PT_INTERP {
            has_interp = true;
        }
        if p_type == PT_LOAD {
            segments.push(ProgramHeader {
                p_type,
                p_flags: u32_at(bytes, off + 4),
                p_offset: u64_at(bytes, off + 8),
                p_vaddr: u64_at(bytes, off + 16),
                p_filesz: u64_at(bytes, off + 32),
                p_memsz: u64_at(bytes, off + 40),
            });
        }
    }

    Ok(ElfFile { e_type, e_entry, segments, has_interp })
}

fn align_down(x: u64, align: u64) -> u64 {
    x & !(align - 1)
}

fn align_up(x: u64, align: u64) -> u64 {
    (x + align - 1) & !(align - 1)
}

/// Load a real ELF64 static-non-PIE (`ET_EXEC`) x86_64 Linux executable
/// from its raw file bytes into a fresh, `VirtualAlloc`-backed `GuestImage`
/// -- real `PT_LOAD` segment bytes copied to their EXACT real vaddr host
/// address, bss zeroed, entry point resolved, and a minimal argv/(empty)
/// envp/(`AT_NULL`-only) auxv stack prepared (see this module's header for
/// exactly what "minimal" means and which real programs it doesn't cover).
///
/// `argv`: the guest program's own `argv` (do not include a trailing NUL --
/// one is added per entry automatically); conventionally `argv[0]` is the
/// program name. `stack_bytes`: how much real, zero-filled, read-write
/// stack space to append after the last loaded segment (same "near the top
/// of the image" placement `GuestImage::new`'s hand-crafted-program path
/// already uses, just with argv/envp/auxv pre-populated instead of left
/// empty).
///
/// Returns a `GuestImage` ready for the UNCHANGED `run_guest` (`lib.rs`) --
/// this function's only job is populating that image and `Cpu`'s initial
/// state (via `GuestImage::entry`/`initial_rsp`); execution afterward goes
/// through the exact same `decode_x86_64` -> `abi_linux_x86_64` ->
/// `intent_map` pipeline every hand-crafted test program already uses.
pub fn load_elf(bytes: &[u8], argv: &[&[u8]], stack_bytes: usize) -> Result<GuestImage, ElfLoadError> {
    let elf = parse_elf64(bytes)?;
    if elf.has_interp {
        return Err(ElfLoadError::HasInterp);
    }
    if elf.e_type != ET_EXEC {
        return Err(ElfLoadError::UnsupportedType { e_type: elf.e_type });
    }
    if elf.segments.is_empty() {
        return Err(ElfLoadError::NoLoadSegments);
    }

    let min_vaddr = elf.segments.iter().map(|s| s.p_vaddr).min().expect("checked non-empty above");
    let max_vaddr = elf.segments.iter().map(|s| s.p_vaddr + s.p_memsz).max().expect("checked non-empty above");
    let region_base = align_down(min_vaddr, PAGE_SIZE);
    let region_end = align_up(max_vaddr, PAGE_SIZE);
    let image_span = (region_end - region_base) as usize;
    let total_len = image_span + stack_bytes;

    let region: MappedRegion =
        mem::alloc_fixed(region_base, total_len).ok_or(ElfLoadError::FixedMapFailed { addr: region_base, len: total_len })?;

    // SAFETY: `region` just committed exactly `total_len` real, zeroed,
    // read-write bytes starting at exactly `region_base` (verified inside
    // `alloc_fixed`) -- `Vec::from_raw_parts` here is sound for that
    // pointer/length/capacity triple. `GuestImage::from_mapped` (below)
    // records `region` alongside `buf` precisely so `GuestImage`'s `Drop`
    // (`lib.rs`) can make sure this `Vec`'s own destructor never runs (it
    // would call the wrong allocator on foreign memory) -- `region`'s own
    // `Drop` does the real `VirtualFree` instead.
    let ptr = region.as_mut_ptr();
    let len = region.len();
    let mut buf = unsafe { Vec::from_raw_parts(ptr, len, len) };

    for seg in &elf.segments {
        let dest_off = (seg.p_vaddr - region_base) as usize;
        let file_start = seg.p_offset as usize;
        let file_end = file_start + seg.p_filesz as usize;
        if file_end > bytes.len() {
            return Err(ElfLoadError::SegmentOutOfFile { p_offset: seg.p_offset, p_filesz: seg.p_filesz, file_len: bytes.len() });
        }
        buf[dest_off..dest_off + seg.p_filesz as usize].copy_from_slice(&bytes[file_start..file_end]);
        // bss tail (memsz - filesz): already zero because `alloc_fixed`
        // just committed fresh pages (Win32 always zero-fills those), but
        // zeroed explicitly too -- see this module's header.
        let bss_start = dest_off + seg.p_filesz as usize;
        let bss_end = dest_off + seg.p_memsz as usize;
        buf[bss_start..bss_end].fill(0);
    }

    let entry = (elf.e_entry - region_base) as usize;
    let base = region_base;
    let initial_rsp = setup_stack(&mut buf, base, image_span, stack_bytes, argv);

    Ok(GuestImage::from_mapped(buf, base, entry, initial_rsp, region))
}

/// Build a minimal `argc`/`argv`/(empty)`envp`/(`AT_NULL`-only)`auxv` stack
/// frame inside `buf`'s stack region (`[image_span, image_span+stack_bytes)`)
/// and return the real host address `rsp` should start at. See this
/// module's header for exactly how minimal this is (empty envp, `AT_NULL`
/// -only auxv -- not full System V ABI auxv).
fn setup_stack(buf: &mut [u8], base: u64, image_span: usize, stack_bytes: usize, argv: &[&[u8]]) -> u64 {
    let stack_top = base + (image_span + stack_bytes) as u64;
    let mut cursor = stack_top;

    fn write_u64(buf: &mut [u8], base: u64, addr: u64, val: u64) {
        let off = (addr - base) as usize;
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    }

    // 1. Argument strings -- placed at the highest addresses (order among
    //    themselves doesn't matter; only the pointer array below needs to
    //    list them in argv order).
    let mut argv_ptrs = Vec::with_capacity(argv.len());
    for a in argv {
        cursor -= (a.len() + 1) as u64;
        let off = (cursor - base) as usize;
        buf[off..off + a.len()].copy_from_slice(a);
        buf[off + a.len()] = 0;
        argv_ptrs.push(cursor);
    }

    // 2. The argc/argv/envp/auxv structure itself, placed just below the
    //    strings, 16-byte aligned (the System V x86_64 ABI's entry-point
    //    stack-alignment requirement -- there is no return address on the
    //    stack at process entry the way there is after a `call`, so `rsp`
    //    itself, not `rsp-8`, must be the aligned value).
    let struct_len = 8                                // argc
        + 8 * (argv_ptrs.len() as u64 + 1)             // argv[0..n] + NULL terminator
        + 8                                            // envp: NULL only (empty envp)
        + 16; // auxv: one AT_NULL {type=0, value=0} pair, terminating an otherwise-empty auxv
    let rsp = (cursor - struct_len) & !0xF;

    let mut w = rsp;
    write_u64(buf, base, w, argv_ptrs.len() as u64); // argc
    w += 8;
    for p in &argv_ptrs {
        write_u64(buf, base, w, *p);
        w += 8;
    }
    write_u64(buf, base, w, 0); // argv[] NULL terminator
    w += 8;
    write_u64(buf, base, w, 0); // envp[] -- just its NULL terminator (empty envp)
    w += 8;
    write_u64(buf, base, w, 0); // auxv[0].a_type = AT_NULL (0)
    w += 8;
    write_u64(buf, base, w, 0); // auxv[0].a_val = 0

    rsp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent_map::ExitMode;

    /// Hand-build the smallest real ELF64 container this parser/loader
    /// should accept -- header + one program header + code, all in a single
    /// `PT_LOAD` segment at `vaddr` -- and return its bytes plus the file
    /// offset the code starts at. This is a from-scratch byte-level ELF
    /// CONTAINER builder (public header/phdr layout, the same knowledge
    /// `parse_elf64` itself documents) used only to test `elf.rs`'s own
    /// parsing/loading mechanics in-process, independent of any external
    /// toolchain -- NOT a substitute for this round's real
    /// compiler-produced-binary verification (see the crate README's ELF
    /// section for that).
    fn build_minimal_elf(vaddr: u64, code: &[u8], entry_off_in_code: u64) -> Vec<u8> {
        const EHDR_LEN: usize = 64;
        const PHDR_LEN: usize = 56;
        let code_off = (EHDR_LEN + PHDR_LEN) as u64;
        let file_len = code_off + code.len() as u64;
        let entry = vaddr + code_off + entry_off_in_code;

        let mut b = vec![0u8; file_len as usize];
        b[0..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        b[4] = ELFCLASS64;
        b[5] = ELFDATA2LSB;
        b[6] = 1; // EI_VERSION
        b[16..18].copy_from_slice(&ET_EXEC.to_le_bytes());
        b[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
        b[20..24].copy_from_slice(&1u32.to_le_bytes()); // e_version
        b[24..32].copy_from_slice(&entry.to_le_bytes());
        b[32..40].copy_from_slice(&EHDR_LEN.to_le_bytes()); // e_phoff
        b[52..54].copy_from_slice(&(EHDR_LEN as u16).to_le_bytes()); // e_ehsize
        b[54..56].copy_from_slice(&(PHDR_LEN as u16).to_le_bytes()); // e_phentsize
        b[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

        let ph = EHDR_LEN;
        b[ph..ph + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
        b[ph + 4..ph + 8].copy_from_slice(&7u32.to_le_bytes()); // PF_R|PF_W|PF_X
        b[ph + 8..ph + 16].copy_from_slice(&0u64.to_le_bytes()); // p_offset
        b[ph + 16..ph + 24].copy_from_slice(&vaddr.to_le_bytes());
        b[ph + 32..ph + 40].copy_from_slice(&file_len.to_le_bytes()); // p_filesz
        b[ph + 40..ph + 48].copy_from_slice(&file_len.to_le_bytes()); // p_memsz

        b[code_off as usize..].copy_from_slice(code);
        b
    }

    #[test]
    fn parses_a_minimal_et_exec_header_and_one_load_segment() {
        let code = [0x90u8]; // nop, irrelevant to this test
        let bytes = build_minimal_elf(0x400000, &code, 0);
        let elf = parse_elf64(&bytes).expect("a well-formed minimal ET_EXEC should parse");
        assert_eq!(elf.e_type, ET_EXEC);
        assert!(!elf.has_interp);
        assert_eq!(elf.segments.len(), 1);
        assert_eq!(elf.segments[0].p_vaddr, 0x400000);
        assert_eq!(elf.e_entry, 0x400000 + 64 + 56);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = build_minimal_elf(0x400000, &[0x90], 0);
        bytes[0] = 0; // corrupt the magic
        match parse_elf64(&bytes) {
            Err(ElfLoadError::BadMagic) => {}
            other => panic!("expected BadMagic, got {other:?}"),
        }
    }

    #[test]
    fn rejects_non_x86_64_machine() {
        let mut bytes = build_minimal_elf(0x400000, &[0x90], 0);
        bytes[18..20].copy_from_slice(&183u16.to_le_bytes()); // EM_AARCH64
        match parse_elf64(&bytes) {
            Err(ElfLoadError::UnsupportedMachine { machine: 183 }) => {}
            other => panic!("expected UnsupportedMachine(183), got {other:?}"),
        }
    }

    /// Real end-to-end: parse + `VirtualAlloc`-map + run a genuinely
    /// hand-assembled (not hand-faked) tiny program through this loader,
    /// landed at its real link vaddr, through the completely UNCHANGED
    /// `run_guest` pipeline. `mov edi, 7` / `mov eax, 60` / `syscall` is
    /// `exit(7)` -- this is still a hand-encoded program (this crate's own
    /// opcode encoder, same technique `testutil::Emitter` uses elsewhere),
    /// it exists to prove `elf.rs`'s OWN mechanics (real `VirtualAlloc` at
    /// the exact vaddr, segment copy, entry resolution, stack setup) in
    /// isolation from an external toolchain's opcode choices -- the crate
    /// README's real-compiler-produced-binary verification is the separate,
    /// stronger bar this round's task actually requires.
    #[test]
    fn loads_and_runs_a_real_et_exec_at_its_real_link_vaddr() {
        let code = [
            0xBF, 0x07, 0x00, 0x00, 0x00, // mov edi, 7
            0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (exit)
            0x0F, 0x05, // syscall
        ];
        let bytes = build_minimal_elf(0x400000, &code, 0);
        let image = match load_elf(&bytes, &[b"prog"], 0x1000) {
            Ok(img) => img,
            Err(ElfLoadError::FixedMapFailed { addr, len }) => {
                // Honest, environment-dependent skip: some other allocation
                // already occupies 0x400000 in this process. Not swallowed
                // silently -- printed so a CI run surfaces it.
                eprintln!("skipping: could not map {len} bytes at {addr:#x} in this process (real, reportable limitation, see elf.rs)");
                return;
            }
            Err(e) => panic!("unexpected ElfLoadError: {e}"),
        };
        assert_eq!(image.base, 0x400000, "ET_EXEC must land at exactly its real link vaddr, not a rebased address");
        assert_eq!(image.entry, 64 + 56, "entry must be the guest-image-relative offset of e_entry");
        let code_byte = image.buf[image.entry];
        assert_eq!(code_byte, 0xBF, "the real ELF code bytes must be present at the resolved entry offset");

        let result = crate::run_guest(&image, ExitMode::Return);
        assert_eq!(result.unwrap(), 7, "exit(7) syscall should propagate through the unchanged run_guest pipeline");
    }
}
