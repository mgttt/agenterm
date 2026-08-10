//! The signature registry — v2 (`plan/archive/design-dynacore-native-core.md` §9),
//! solving the specific gap Q23 measured (`research/dynamic-core/runtime-intent/
//! RESULTS.md`, S5): a single-author pack has no way to make `verify()` know
//! a native symbol's TRUE arity, because Windows x64 exports carry no
//! queryable ABI metadata. Recompiling `agenterm.exe` to add an eighth
//! `Intent` "worked" only because `Intent::contract_arity()` (`ir.rs`) is an
//! INDEPENDENT SECOND PARTY's assertion (a human read the Win32 docs, wrote
//! a constant, and it went through code review) — not because compilation
//! itself proves anything about the ABI.
//!
//! This table is that same second-party assertion, paid ONCE PER SYMBOL
//! instead of once per whole-crate recompile: a human reads the real ABI,
//! adds one row here, and the row goes through the same git + code review
//! process any other change to this crate does. `verify.rs` NEVER trusts a
//! pack's own declared arity for a registry-backed call — it looks the
//! symbol up here and DERIVES the contract from this table, exactly Q23's
//! "derive, not declare" finding (S3/S4).
//!
//! **This is a mechanism proof, not a distribution/signing pipeline.**
//! Per design doc §9.3, this round deliberately does NOT build: runtime
//! hot-reload of this table, a separate signed/distributed registry file, or
//! any way for a pack itself to add an entry. The only way a new symbol is
//! ever admitted is a human editing THIS source file and it going through
//! ordinary code review — that human step *is* the registry's entire
//! security model, stated plainly rather than implied.
//!
//! Seeded with the same three real kernel32 exports the Q23 research
//! prototype used (`research/dynamic-core/runtime-intent/main.rs`, S1/S2) —
//! a genuine demonstration against real Win32 APIs, not placeholder data.
//! All three are documented by Microsoft to return a 32-bit value and take
//! only integer/pointer-width arguments, within the 0..=4-arg range this
//! crate's registry-backed call trampoline supports (`seam.rs`,
//! design doc §9.3).

#![allow(dead_code)]

/// One human-reviewed row: a native export this registry vouches the real
/// arity for. See this module's header for what "vouches" means here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignatureEntry {
    pub symbol: &'static str,
    pub module: &'static str,
    /// The symbol's REAL native-call arity — a human-verified fact about the
    /// actual Win32 ABI, independent of anything any pack ever says.
    pub arity: usize,
}

/// The compiled-in signature registry. `agenterm-nativecore`'s only source
/// of truth for "is this native export reachable through the registry-backed
/// call path, and with how many arguments" (design doc §9). See `lookup`.
///
/// | symbol | module | arity | real signature |
/// |---|---|---|---|
/// | `MulDiv` | `kernel32.dll` | 3 | `int MulDiv(int, int, int)` |
/// | `lstrlenA` | `kernel32.dll` | 1 | `int lstrlenA(LPCSTR)` |
/// | `GetTickCount` | `kernel32.dll` | 0 | `DWORD GetTickCount(void)` |
pub const SIGNATURE_REGISTRY: &[SignatureEntry] = &[
    SignatureEntry { symbol: "MulDiv", module: "kernel32.dll", arity: 3 },
    SignatureEntry { symbol: "lstrlenA", module: "kernel32.dll", arity: 1 },
    SignatureEntry { symbol: "GetTickCount", module: "kernel32.dll", arity: 0 },
];

/// Look up `symbol` in `module` within the registry. Requires an EXACT match
/// on both fields — a pack cannot get a real symbol name admitted against a
/// module the registry did not vouch for it in (closes a naming loophole:
/// claiming `MulDiv` lives in some other DLL would not pass). Returns the
/// registry entry (and therefore the registry-DERIVED arity) on a hit, or
/// `None` — which `verify.rs` turns into `IrFault::SymbolNotInRegistry`,
/// never a silent pass.
pub fn lookup(module: &str, symbol: &str) -> Option<&'static SignatureEntry> {
    SIGNATURE_REGISTRY.iter().find(|e| e.module == module && e.symbol == symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_entries_are_found_by_exact_module_and_symbol() {
        for entry in SIGNATURE_REGISTRY {
            assert_eq!(lookup(entry.module, entry.symbol), Some(entry));
        }
    }

    #[test]
    fn unknown_symbol_is_not_found() {
        assert_eq!(lookup("kernel32.dll", "OutputDebugStringA"), None);
    }

    #[test]
    fn known_symbol_in_the_wrong_module_is_not_found() {
        // MulDiv is real, but not in this (fictional) module -- lookup must
        // not fall back to matching on symbol name alone.
        assert_eq!(lookup("not-a-real-module.dll", "MulDiv"), None);
    }
}
