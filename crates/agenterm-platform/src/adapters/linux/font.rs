use crate::contract::font::FontFileCandidate;

pub(crate) fn candidates() -> Vec<FontFileCandidate> {
    vec![
        FontFileCandidate {
            name: "DejaVu Sans Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "dejavu",
                "DejaVuSansMono.ttf",
            ],
        },
        FontFileCandidate {
            name: "Liberation Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "liberation",
                "LiberationMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Liberation Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "liberation2",
                "LiberationMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Noto Sans Mono",
            components: &[
                "usr",
                "share",
                "fonts",
                "truetype",
                "noto",
                "NotoSansMono-Regular.ttf",
            ],
        },
        FontFileCandidate {
            name: "Noto Sans Mono CJK",
            components: &[
                "usr",
                "share",
                "fonts",
                "opentype",
                "noto",
                "NotoSansCJK-Regular.ttc",
            ],
        },
    ]
}
