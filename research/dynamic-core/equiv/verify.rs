//! The equivalence invariant, made STRUCTURAL (spec §1.1).
//!
//! `VerifiedArtifact` has private byte fields and exactly ONE construction entry
//! (`build` / `build_from`), both of which run `check_congruence` and REFUSE to
//! produce the value on a violation. Downstream code can only obtain runnable bytes
//! through `win_bytes()`/`sysv_bytes()`, which require a `VerifiedArtifact` — so the
//! equivalence check is on the critical path of "produce executable bytes" and cannot
//! be forgotten. Contrast the thing this beats: a separable `check_equiv` script run
//! after the build, which can simply be skipped.

use crate::common::{lower_regions, Region, RegionKind, Target};
use crate::ir::Module;

pub struct Lowered {
    pub bytes: Vec<u8>,
    pub regions: Vec<Region>,
}

pub fn lower_one(m: &Module, t: &dyn Target) -> Lowered {
    let (bytes, regions) = lower_regions(m, t);
    Lowered { bytes, regions }
}

#[derive(Debug)]
pub enum EquivViolation {
    /// the two lowerings did not even produce the same region sequence — a structural
    /// break (should be impossible from one IR; a mutated lowerer could cause it)
    RegionCountMismatch { sysv: usize, win: usize },
    KindMismatch { idx: usize, sysv: RegionKind, win: RegionKind },
    /// a Neutral region differs byte-for-byte — the invariant's teeth
    NeutralBytesDiffer { idx: usize },
    /// a Control transfer targets a different block on the two sides
    ControlTargetDiffers { idx: usize, sysv: u32, win: u32 },
}

/// Per-target byte accounting by region kind (criterion ②).
#[derive(Clone, Copy, Default, Debug)]
pub struct Coverage {
    pub neutral: usize,
    pub control: usize,
    pub frame: usize,
    pub ctx: usize,
    pub intent: usize,
    pub total: usize,
}
impl Coverage {
    fn of(l: &Lowered) -> Coverage {
        let mut c = Coverage::default();
        for r in &l.regions {
            let n = r.len();
            match r.kind {
                RegionKind::Neutral => c.neutral += n,
                RegionKind::Control(_) => c.control += n,
                RegionKind::Frame => c.frame += n,
                RegionKind::CtxReg => c.ctx += n,
                RegionKind::Intent(_) => c.intent += n,
            }
            c.total += n;
        }
        c
    }
    /// Tier A: straight-line neutral code, byte-identical (the airtight memcmp).
    pub fn neutral_frac(&self) -> f64 {
        self.neutral as f64 / self.total as f64
    }
    /// Tier A′: neutral + control-by-label (structural congruence).
    pub fn struct_frac(&self) -> f64 {
        (self.neutral + self.control) as f64 / self.total as f64
    }
    /// ABI mechanics divergence (same opcode, ABI-mandated operand): frame size + ctx reg.
    pub fn mechanics_frac(&self) -> f64 {
        (self.frame + self.ctx) as f64 / self.total as f64
    }
    /// the OS-interface leak: bytes with NO structural equivalence (need Tier B/C).
    pub fn intent_frac(&self) -> f64 {
        self.intent as f64 / self.total as f64
    }
}

/// THE INVARIANT. Walks the two region sequences in lockstep (both are driven by the
/// same IR walk, so they must have identical kind-sequences) and enforces:
///   * Neutral regions are byte-identical;
///   * Control regions target the same block (rel32 bytes may differ from drift);
///   * Frame / CtxReg / Intent regions are quarantined (not byte-checked here).
pub fn check_congruence(sysv: &Lowered, win: &Lowered) -> Result<(), EquivViolation> {
    if sysv.regions.len() != win.regions.len() {
        return Err(EquivViolation::RegionCountMismatch {
            sysv: sysv.regions.len(),
            win: win.regions.len(),
        });
    }
    for (idx, (rs, rw)) in sysv.regions.iter().zip(win.regions.iter()).enumerate() {
        match (rs.kind, rw.kind) {
            (RegionKind::Neutral, RegionKind::Neutral) => {
                let bs = &sysv.bytes[rs.start..rs.end];
                let bw = &win.bytes[rw.start..rw.end];
                if bs != bw {
                    return Err(EquivViolation::NeutralBytesDiffer { idx });
                }
            }
            (RegionKind::Control(a), RegionKind::Control(b)) => {
                if a != b {
                    return Err(EquivViolation::ControlTargetDiffers { idx, sysv: a, win: b });
                }
            }
            (RegionKind::Frame, RegionKind::Frame)
            | (RegionKind::CtxReg, RegionKind::CtxReg) => { /* mechanics quarantine */ }
            (RegionKind::Intent(a), RegionKind::Intent(b)) if a == b => { /* leak quarantine */ }
            _ => {
                return Err(EquivViolation::KindMismatch { idx, sysv: rs.kind, win: rw.kind });
            }
        }
    }
    Ok(())
}

/// A pair of lowerings that HAVE PASSED the congruence invariant. Private fields: the
/// only way to get one is `build`/`build_from`, so the check always ran.
pub struct VerifiedArtifact {
    sysv: Lowered,
    win: Lowered,
    identical: bool,
}

impl VerifiedArtifact {
    pub fn build(m: &Module, sysv_t: &dyn Target, win_t: &dyn Target) -> Result<Self, EquivViolation> {
        let sysv = lower_one(m, sysv_t);
        let win = lower_one(m, win_t);
        Self::build_from(sysv, win)
    }
    /// build from pre-lowered halves — used by the negative test to inject a mutant.
    pub fn build_from(sysv: Lowered, win: Lowered) -> Result<Self, EquivViolation> {
        check_congruence(&sysv, &win)?;
        let identical = sysv.bytes == win.bytes;
        Ok(VerifiedArtifact { sysv, win, identical })
    }
    pub fn win_bytes(&self) -> &[u8] {
        &self.win.bytes
    }
    pub fn sysv_bytes(&self) -> &[u8] {
        &self.sysv.bytes
    }
    /// whole-artifact byte identity (Tier A over the entire image; true only when no
    /// region diverged at all — i.e. pure_compute)
    pub fn identical(&self) -> bool {
        self.identical
    }
    pub fn sysv_coverage(&self) -> Coverage {
        Coverage::of(&self.sysv)
    }
    pub fn win_coverage(&self) -> Coverage {
        Coverage::of(&self.win)
    }
}
