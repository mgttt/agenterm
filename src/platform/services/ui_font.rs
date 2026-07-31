//! OS-neutral font candidate service.

use crate::platform::{contract::ui_font::FontFileCandidate, selected};

pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    selected::ui_font::candidates()
}
