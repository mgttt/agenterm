use crate::contract::font::FontFileCandidate;

pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    vec![
        FontFileCandidate {
            name: "SF Mono",
            components: &["System", "Library", "Fonts", "SFNSMono.ttf"],
        },
        FontFileCandidate {
            name: "Hiragino Sans GB",
            components: &["System", "Library", "Fonts", "Hiragino Sans GB.ttc"],
        },
        FontFileCandidate {
            name: "Apple Symbols",
            components: &["System", "Library", "Fonts", "Apple Symbols.ttf"],
        },
    ]
}
