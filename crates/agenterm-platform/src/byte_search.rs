//! Architecture-specialized substring search over raw bytes.
//!
//! The scan itself is portable, but the inner loop is where `agenterm-con`'s
//! wait-text matching spends its time, so x86_64 gets a hand-written kernel and
//! every other architecture gets the equivalent scalar loop. That split is a
//! per-architecture mechanic, which is why it lives in the platform crate
//! rather than in a binary under `src/**` — the source-boundary suites bar
//! compile-time architecture branching there, and correctly so: an entrypoint
//! may select a subsystem, but it may not carry machine-level implementations.

/// Does `haystack` contain `needle`?
///
/// An empty needle matches, matching `str::contains`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    let Some(starts) = haystack
        .len()
        .checked_sub(needle.len())
        .map(|last| last + 1)
    else {
        return false;
    };
    // SAFETY: `starts` is `haystack.len() - needle.len() + 1`, so every
    // `haystack[start + index]` read for `start < starts` and
    // `index < needle.len()` stays in bounds of both slices.
    unsafe { contains_kernel(haystack.as_ptr(), starts, needle.as_ptr(), needle.len()) }
}

#[cfg(target_arch = "x86_64")]
unsafe fn contains_kernel(
    candidate: *const u8,
    starts: usize,
    needle: *const u8,
    needle_len: usize,
) -> bool {
    use core::arch::asm;

    let result: usize;
    let _index: usize;
    let _left: usize;
    let _right: usize;
    unsafe {
        asm!(
            "2:",
            "test {starts}, {starts}",
            "jz 5f",
            "xor {index}, {index}",
            "3:",
            "cmp {index}, {needle_len}",
            "je 4f",
            "movzx {left:e}, byte ptr [{candidate} + {index}]",
            "movzx {right:e}, byte ptr [{needle} + {index}]",
            "cmp {left:e}, {right:e}",
            "jne 6f",
            "inc {index}",
            "jmp 3b",
            "6:",
            "inc {candidate}",
            "dec {starts}",
            "jmp 2b",
            "4:",
            "mov {result}, 1",
            "jmp 7f",
            "5:",
            "xor {result}, {result}",
            "7:",
            candidate = inout(reg) candidate => _,
            starts = inout(reg) starts => _,
            needle = in(reg) needle,
            needle_len = in(reg) needle_len,
            result = out(reg) result,
            index = out(reg) _index,
            left = out(reg) _left,
            right = out(reg) _right,
            options(nostack, readonly)
        );
    }
    result != 0
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn contains_kernel(
    haystack: *const u8,
    starts: usize,
    needle: *const u8,
    needle_len: usize,
) -> bool {
    for start in 0..starts {
        let mut index = 0;
        while index < needle_len {
            if unsafe { *haystack.add(start + index) != *needle.add(index) } {
                break;
            }
            index += 1;
        }
        if index == needle_len {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::contains;

    #[test]
    fn matches_the_standard_library_on_every_window() {
        // Both kernels are index arithmetic around a byte compare, so the
        // failure modes are off-by-one at the ends. Compare against
        // slice::windows over every needle of every haystack in a small
        // alphabet, which puts a case on each boundary rather than near it.
        let haystacks: [&[u8]; 6] = [b"", b"a", b"ab", b"aab", b"abcabd", b"aaaa"];
        let needles: [&[u8]; 8] = [b"", b"a", b"b", b"ab", b"aa", b"abd", b"abcabd", b"abcabde"];
        for haystack in haystacks {
            for needle in needles {
                let expected = if needle.is_empty() {
                    true
                } else {
                    haystack.len() >= needle.len()
                        && haystack
                            .windows(needle.len())
                            .any(|window| window == needle)
                };
                assert_eq!(
                    contains(haystack, needle),
                    expected,
                    "haystack={haystack:?} needle={needle:?}"
                );
            }
        }
    }

    #[test]
    fn a_needle_longer_than_the_haystack_never_matches() {
        assert!(!contains(b"ab", b"abc"));
        assert!(contains(b"", b""));
        assert!(!contains(b"", b"a"));
    }
}
