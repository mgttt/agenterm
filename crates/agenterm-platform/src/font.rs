//! OS-neutral font candidate service.

pub use crate::contract::font::FontFileCandidate;
use crate::selected;

pub fn candidates() -> Vec<FontFileCandidate> {
    selected::font::candidates()
}
