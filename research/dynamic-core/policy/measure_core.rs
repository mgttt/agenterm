//! Q15 ③ byte measurement — the POLICY interpreter body, mirrored on Q9's measure_core.rs so
//! the delta over Q9's 1908 B eval-core is honest (same toolchain, same method).
//!   * default build              -> `.text` = policy eval-core + OS seam
//!   * `--cfg policy_measure_core` -> do_intent collapses to `unreachable`; `.text` ~= policy
//!     eval-core alone. Compare to Q9 `--cfg interp_measure_core` = 1908 B.

#[path = "../ir/spec/ir.rs"]
mod ir;
#[path = "interp_policy.rs"]
mod interp_policy;

pub use interp_policy::run_policy;
