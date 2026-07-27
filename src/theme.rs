use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

/// The stable identifier persisted in settings. Palette details are deliberately
/// not part of the settings file so built-in palettes can evolve independently.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum ThemeId {
    #[default]
    Dark,
    Light,
}

impl ThemeId {
    pub(crate) const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub(crate) const fn palette(self) -> &'static ThemePalette {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }
}

impl Serialize for ThemeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThemeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ThemeIdVisitor;

        impl Visitor<'_> for ThemeIdVisitor {
            type Value = ThemeId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a built-in theme ID string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                // Settings are long-lived. An ID written by a newer version must
                // not prevent older versions from loading unrelated settings.
                Ok(match value {
                    "light" => ThemeId::Light,
                    _ => ThemeId::Dark,
                })
            }
        }

        deserializer.deserialize_str(ThemeIdVisitor)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Rgb {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
}

impl Rgb {
    pub(crate) const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    /// Win32 COLORREF representation, kept here to avoid a Windows API
    /// dependency in the theme model.
    pub(crate) const fn colorref(self) -> u32 {
        self.red as u32 | ((self.green as u32) << 8) | ((self.blue as u32) << 16)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ThemePalette {
    // Host-owned surfaces and typography.
    pub(crate) sidebar: Rgb,
    pub(crate) terminal_background: Rgb,
    pub(crate) composer: Rgb,
    pub(crate) modal: Rgb,
    pub(crate) status: Rgb,
    pub(crate) text: Rgb,
    pub(crate) muted_text: Rgb,
    pub(crate) divider: Rgb,

    // Native controls and interaction states.
    pub(crate) control: Rgb,
    pub(crate) control_hover: Rgb,
    pub(crate) control_pressed: Rgb,
    pub(crate) active: Rgb,
    pub(crate) active_border: Rgb,
    pub(crate) focus_ring: Rgb,

    // Semantic accents.
    pub(crate) success: Rgb,
    pub(crate) warning: Rgb,
    pub(crate) danger: Rgb,
    pub(crate) accent: Rgb,

    // Terminal-owned defaults and host selection.
    pub(crate) terminal_foreground: Rgb,
    pub(crate) selection_background: Rgb,
    pub(crate) selection_foreground: Rgb,
    pub(crate) scrollbar_track: Rgb,
    pub(crate) scrollbar_thumb: Rgb,
    pub(crate) scrollbar_thumb_active: Rgb,
    pub(crate) ansi: [Rgb; 16],
}

const fn rgb(red: u8, green: u8, blue: u8) -> Rgb {
    Rgb::new(red, green, blue)
}

// Every existing host color and ANSI entry is retained exactly. Newly explicit
// interaction fields alias the colors used for those states before theming.
pub(crate) const DARK: ThemePalette = ThemePalette {
    sidebar: rgb(24, 27, 34),
    terminal_background: rgb(12, 14, 18),
    composer: rgb(31, 35, 44),
    modal: rgb(38, 43, 54),
    status: rgb(19, 22, 28),
    text: rgb(214, 220, 230),
    muted_text: rgb(145, 153, 168),
    divider: rgb(82, 94, 112),
    control: rgb(31, 35, 44),
    control_hover: rgb(42, 49, 61),
    control_pressed: rgb(38, 43, 54),
    active: rgb(42, 49, 61),
    active_border: rgb(76, 94, 122),
    focus_ring: rgb(245, 190, 100),
    success: rgb(121, 215, 135),
    warning: rgb(245, 190, 100),
    danger: rgb(240, 100, 95),
    accent: rgb(100, 155, 235),
    terminal_foreground: rgb(214, 220, 230),
    selection_background: rgb(100, 155, 235),
    selection_foreground: rgb(12, 14, 18),
    scrollbar_track: rgb(19, 22, 28),
    scrollbar_thumb: rgb(82, 94, 112),
    scrollbar_thumb_active: rgb(42, 49, 61),
    ansi: [
        rgb(12, 14, 18),
        rgb(205, 73, 69),
        rgb(91, 184, 104),
        rgb(220, 184, 87),
        rgb(84, 132, 214),
        rgb(176, 101, 193),
        rgb(69, 179, 184),
        rgb(214, 220, 230),
        rgb(100, 108, 123),
        rgb(240, 100, 95),
        rgb(121, 215, 135),
        rgb(245, 210, 112),
        rgb(112, 159, 236),
        rgb(205, 132, 222),
        rgb(97, 211, 216),
        rgb(255, 255, 255),
    ],
};

pub(crate) const LIGHT: ThemePalette = ThemePalette {
    sidebar: rgb(238, 241, 246),
    terminal_background: rgb(250, 251, 253),
    composer: rgb(245, 247, 250),
    modal: rgb(255, 255, 255),
    status: rgb(229, 233, 240),
    text: rgb(31, 38, 49),
    muted_text: rgb(88, 98, 113),
    divider: rgb(174, 183, 197),
    control: rgb(245, 247, 250),
    control_hover: rgb(224, 230, 239),
    control_pressed: rgb(210, 218, 230),
    active: rgb(218, 227, 241),
    active_border: rgb(91, 112, 143),
    focus_ring: rgb(137, 79, 0),
    success: rgb(25, 116, 55),
    warning: rgb(137, 79, 0),
    danger: rgb(176, 42, 42),
    accent: rgb(35, 94, 168),
    terminal_foreground: rgb(31, 38, 49),
    selection_background: rgb(35, 94, 168),
    selection_foreground: rgb(255, 255, 255),
    scrollbar_track: rgb(229, 233, 240),
    scrollbar_thumb: rgb(145, 156, 173),
    scrollbar_thumb_active: rgb(91, 112, 143),
    ansi: [
        rgb(31, 38, 49),
        rgb(176, 42, 42),
        rgb(25, 116, 55),
        rgb(137, 79, 0),
        rgb(35, 94, 168),
        rgb(130, 67, 152),
        rgb(0, 112, 117),
        rgb(222, 226, 232),
        rgb(88, 98, 113),
        rgb(211, 61, 61),
        rgb(38, 139, 69),
        rgb(166, 99, 0),
        rgb(57, 116, 193),
        rgb(155, 87, 178),
        rgb(0, 137, 143),
        rgb(255, 255, 255),
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_ids_have_stable_strings_and_labels() {
        assert_eq!(ThemeId::ALL.map(ThemeId::as_str), ["dark", "light"]);
        assert_eq!(ThemeId::ALL.map(ThemeId::label), ["Dark", "Light"]);
        assert_eq!(serde_json::to_string(&ThemeId::Dark).unwrap(), "\"dark\"");
        assert_eq!(
            serde_json::from_str::<ThemeId>("\"light\"").unwrap(),
            ThemeId::Light
        );
    }

    #[test]
    fn unknown_theme_id_migrates_to_dark() {
        assert_eq!(
            serde_json::from_str::<ThemeId>("\"future-theme\"").unwrap(),
            ThemeId::Dark
        );
        assert!(serde_json::from_str::<ThemeId>("42").is_err());
    }

    #[test]
    fn dark_palette_retains_the_pre_theme_colors() {
        assert_eq!(DARK.sidebar, rgb(24, 27, 34));
        assert_eq!(DARK.terminal_background, rgb(12, 14, 18));
        assert_eq!(DARK.composer, rgb(31, 35, 44));
        assert_eq!(DARK.text, rgb(214, 220, 230));
        assert_eq!(DARK.selection_background, rgb(100, 155, 235));
        assert_eq!(DARK.ansi[0], rgb(12, 14, 18));
        assert_eq!(DARK.ansi[15], rgb(255, 255, 255));
    }

    #[test]
    fn palettes_expose_terminal_and_interaction_colors() {
        for theme in ThemeId::ALL {
            let palette = theme.palette();
            assert_ne!(palette.terminal_foreground, palette.terminal_background);
            assert_ne!(palette.selection_foreground, palette.selection_background);
            assert_ne!(palette.focus_ring, palette.sidebar);
            assert_eq!(palette.ansi.len(), 16);
        }
    }

    #[test]
    fn light_palette_is_fixed_and_keeps_key_text_pairs_readable() {
        assert_eq!(LIGHT.sidebar, rgb(238, 241, 246));
        assert_eq!(LIGHT.terminal_background, rgb(250, 251, 253));
        assert_eq!(LIGHT.text, rgb(31, 38, 49));
        assert_eq!(LIGHT.accent, rgb(35, 94, 168));
        assert_eq!(LIGHT.ansi[0], rgb(31, 38, 49));
        assert_eq!(LIGHT.ansi[15], rgb(255, 255, 255));

        assert!(contrast(LIGHT.text, LIGHT.sidebar) >= 4.5);
        assert!(contrast(LIGHT.text, LIGHT.composer) >= 4.5);
        assert!(contrast(LIGHT.terminal_foreground, LIGHT.terminal_background) >= 4.5);
        assert!(contrast(LIGHT.muted_text, LIGHT.sidebar) >= 4.5);
        assert!(contrast(LIGHT.selection_foreground, LIGHT.selection_background) >= 4.5);
        assert!(contrast(LIGHT.focus_ring, LIGHT.sidebar) >= 4.5);
    }

    #[test]
    fn rgb_converts_to_win32_colorref_byte_order() {
        assert_eq!(rgb(0x12, 0x34, 0x56).colorref(), 0x0056_3412);
    }

    fn contrast(left: Rgb, right: Rgb) -> f64 {
        let (lighter, darker) = {
            let left = luminance(left);
            let right = luminance(right);
            (left.max(right), left.min(right))
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    fn luminance(color: Rgb) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.red) + 0.7152 * channel(color.green) + 0.0722 * channel(color.blue)
    }
}
