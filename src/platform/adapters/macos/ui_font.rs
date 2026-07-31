use crate::platform::contract::ui_font::FontFileCandidate;

pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    crate::platform::selected::native::font::candidates()
        .iter()
        .map(|candidate| FontFileCandidate {
            name: candidate.name,
            components: candidate.components,
        })
        .collect()
}
