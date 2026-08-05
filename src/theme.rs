use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

use crate::locale::{LocaleId, UiText};

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

    pub(crate) const fn appearance_preset(self) -> AppearancePreset {
        AppearancePreset::from_theme_id(self)
    }
}

/// Built-in skin family. Orthogonal to [`Luminance`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum SkinId {
    #[default]
    Classic,
    Fancy,
}

impl SkinId {
    pub(crate) const ALL: [Self; 2] = [Self::Classic, Self::Fancy];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Fancy => "fancy",
        }
    }
}

impl Serialize for SkinId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SkinId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SkinIdVisitor;

        impl Visitor<'_> for SkinIdVisitor {
            type Value = SkinId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a built-in skin ID string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match value {
                    "fancy" => SkinId::Fancy,
                    _ => SkinId::Classic,
                })
            }
        }

        deserializer.deserialize_str(SkinIdVisitor)
    }
}

/// Day/night luminance paired with [`SkinId`] for composite appearance presets.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Luminance {
    Day,
    #[default]
    Night,
}

impl Luminance {
    pub(crate) const ALL: [Self; 2] = [Self::Day, Self::Night];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Night => "night",
        }
    }

    pub(crate) const fn color_theme(self) -> ThemeId {
        match self {
            Self::Day => ThemeId::Light,
            Self::Night => ThemeId::Dark,
        }
    }
}

impl Serialize for Luminance {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Luminance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LuminanceVisitor;

        impl Visitor<'_> for LuminanceVisitor {
            type Value = Luminance;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a built-in luminance ID string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match value {
                    "day" | "light" => Luminance::Day,
                    _ => Luminance::Night,
                })
            }
        }

        deserializer.deserialize_str(LuminanceVisitor)
    }
}

/// Composite built-in appearance preset `{skin}-{luminance}`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AppearancePreset {
    skin: SkinId,
    luminance: Luminance,
}

impl AppearancePreset {
    pub(crate) const ALL: [Self; 4] = [
        Self::classic_day(),
        Self::classic_night(),
        Self::fancy_day(),
        Self::fancy_night(),
    ];

    pub(crate) const fn classic_day() -> Self {
        Self {
            skin: SkinId::Classic,
            luminance: Luminance::Day,
        }
    }

    pub(crate) const fn classic_night() -> Self {
        Self {
            skin: SkinId::Classic,
            luminance: Luminance::Night,
        }
    }

    pub(crate) const fn fancy_day() -> Self {
        Self {
            skin: SkinId::Fancy,
            luminance: Luminance::Day,
        }
    }

    pub(crate) const fn fancy_night() -> Self {
        Self {
            skin: SkinId::Fancy,
            luminance: Luminance::Night,
        }
    }

    pub(crate) const fn skin(self) -> SkinId {
        self.skin
    }

    pub(crate) const fn luminance(self) -> Luminance {
        self.luminance
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match (self.skin, self.luminance) {
            (SkinId::Classic, Luminance::Day) => "classic-day",
            (SkinId::Classic, Luminance::Night) => "classic-night",
            (SkinId::Fancy, Luminance::Day) => "fancy-day",
            (SkinId::Fancy, Luminance::Night) => "fancy-night",
        }
    }

    pub(crate) const fn color_theme(self) -> ThemeId {
        self.luminance.color_theme()
    }

    pub(crate) fn palette(self) -> &'static ThemePalette {
        embedded_palettes().for_preset(self)
    }

    pub(crate) const fn from_theme_id(theme: ThemeId) -> Self {
        match theme {
            ThemeId::Light => Self::classic_day(),
            ThemeId::Dark => Self::classic_night(),
        }
    }

    pub(crate) const fn ui_text_label(self) -> UiText {
        match (self.skin, self.luminance) {
            (SkinId::Classic, Luminance::Day) => UiText::PresetClassicDay,
            (SkinId::Classic, Luminance::Night) => UiText::PresetClassicNight,
            (SkinId::Fancy, Luminance::Day) => UiText::PresetFancyDay,
            (SkinId::Fancy, Luminance::Night) => UiText::PresetFancyNight,
        }
    }

    pub(crate) const fn ui_text_description(self) -> UiText {
        match (self.skin, self.luminance) {
            (SkinId::Classic, Luminance::Day) => UiText::PresetClassicDayDesc,
            (SkinId::Classic, Luminance::Night) => UiText::PresetClassicNightDesc,
            (SkinId::Fancy, Luminance::Day) => UiText::PresetFancyDayDesc,
            (SkinId::Fancy, Luminance::Night) => UiText::PresetFancyNightDesc,
        }
    }

    pub(crate) const fn label(self, locale: LocaleId) -> &'static str {
        locale.text(self.ui_text_label())
    }

    pub(crate) fn description(self, locale: LocaleId) -> &'static str {
        embedded_descriptions()
            .description(self.as_str(), locale)
            .unwrap_or_else(|| locale.text(self.ui_text_description()))
    }

    pub(crate) fn skin_metrics(self) -> &'static SkinMetrics {
        &embedded_manifest(self.skin()).metrics
    }

    pub(crate) fn window_title(self, version: &str, instance: Option<&str>) -> String {
        embedded_manifest(self.skin()).format_title(version, instance)
    }

    pub(crate) fn window_icon_png(self) -> &'static [u8] {
        match self.skin() {
            SkinId::Fancy => include_bytes!("../assets/skins/fancy/icon.png"),
            SkinId::Classic => include_bytes!("../assets/agenterm-icon.png"),
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "classic-day" | "light" => Self::classic_day(),
            "classic-night" | "dark" => Self::classic_night(),
            "fancy-day" => Self::fancy_day(),
            "fancy-night" => Self::fancy_night(),
            _ => Self::classic_night(),
        }
    }
}

impl Serialize for AppearancePreset {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AppearancePreset {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AppearancePresetVisitor;

        impl Visitor<'_> for AppearancePresetVisitor {
            type Value = AppearancePreset;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a built-in appearance preset ID string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(AppearancePreset::parse(value))
            }
        }

        deserializer.deserialize_str(AppearancePresetVisitor)
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SkinMetrics {
    pub(crate) corner_radius_control_px: u8,
    pub(crate) corner_radius_modal_px: u8,
    pub(crate) border_width_px: u8,
    pub(crate) scrollbar_style: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EmbeddedManifest {
    brand_short: &'static str,
    title_template: &'static str,
    title_template_with_instance: &'static str,
    metrics: SkinMetrics,
}

impl EmbeddedManifest {
    fn format_title(self, version: &str, instance: Option<&str>) -> String {
        let template = if instance.is_some() {
            self.title_template_with_instance
        } else {
            self.title_template
        };
        let mut title = template
            .replace("{brand}", self.brand_short)
            .replace("{version}", version);
        if let Some(instance) = instance.filter(|value| !value.is_empty()) {
            title = title.replace("{instance}", instance);
        }
        title
    }
}

struct EmbeddedPalettes {
    classic_day: ThemePalette,
    classic_night: ThemePalette,
    fancy_day: ThemePalette,
    fancy_night: ThemePalette,
}

impl EmbeddedPalettes {
    fn for_preset(&self, preset: AppearancePreset) -> &ThemePalette {
        match (preset.skin(), preset.luminance()) {
            (SkinId::Classic, Luminance::Day) => &self.classic_day,
            (SkinId::Classic, Luminance::Night) => &self.classic_night,
            (SkinId::Fancy, Luminance::Day) => &self.fancy_day,
            (SkinId::Fancy, Luminance::Night) => &self.fancy_night,
        }
    }
}

struct PresetDescriptions {
    entries: [(&'static str, LocalizedCopy); 4],
}

impl PresetDescriptions {
    fn description(&self, preset_id: &str, locale: LocaleId) -> Option<&'static str> {
        self.entries
            .iter()
            .find(|(id, _)| *id == preset_id)
            .map(|(_, copy)| copy.for_locale(locale))
    }
}

struct LocalizedCopy {
    en: &'static str,
    zh_hant: &'static str,
}

impl LocalizedCopy {
    fn for_locale(&self, locale: LocaleId) -> &'static str {
        match locale {
            LocaleId::TraditionalChinese => self.zh_hant,
            LocaleId::English => self.en,
        }
    }
}

#[derive(Deserialize)]
struct PaletteFile {
    colors: PaletteColors,
}

#[derive(Deserialize)]
struct PaletteColors {
    sidebar: String,
    terminal_background: String,
    composer: String,
    modal: String,
    status: String,
    text: String,
    muted_text: String,
    divider: String,
    control: String,
    control_hover: String,
    control_pressed: String,
    active: String,
    active_border: String,
    focus_ring: String,
    success: String,
    warning: String,
    danger: String,
    accent: String,
    terminal_foreground: String,
    selection_background: String,
    selection_foreground: String,
    scrollbar_track: String,
    scrollbar_thumb: String,
    scrollbar_thumb_active: String,
    ansi: Vec<String>,
}

#[derive(Deserialize)]
struct SkinManifestFile {
    brand_short: String,
    title_template: String,
    title_template_with_instance: String,
    metrics: SkinMetricsFile,
}

#[derive(Deserialize)]
struct SkinMetricsFile {
    corner_radius_control_px: u8,
    corner_radius_modal_px: u8,
    border_width_px: u8,
    scrollbar_style: String,
}

#[derive(Deserialize)]
struct SettingsDescriptionsFile {
    #[serde(rename = "classic-day")]
    classic_day: LocalizedCopyFile,
    #[serde(rename = "classic-night")]
    classic_night: LocalizedCopyFile,
    #[serde(rename = "fancy-day")]
    fancy_day: LocalizedCopyFile,
    #[serde(rename = "fancy-night")]
    fancy_night: LocalizedCopyFile,
}

#[derive(Deserialize)]
struct LocalizedCopyFile {
    en: String,
    #[serde(rename = "zh-Hant")]
    zh_hant: String,
}

fn embedded_palettes() -> &'static EmbeddedPalettes {
    static PALETTES: OnceLock<&'static EmbeddedPalettes> = OnceLock::new();
    PALETTES.get_or_init(|| {
        Box::leak(Box::new(EmbeddedPalettes {
            classic_day: palette_from_json(include_str!(
                "../assets/skins/classic/palettes/day.json"
            )),
            classic_night: palette_from_json(include_str!(
                "../assets/skins/classic/palettes/night.json"
            )),
            fancy_day: palette_from_json(include_str!(
                "../assets/skins/fancy/palettes/day.json"
            )),
            fancy_night: palette_from_json(include_str!(
                "../assets/skins/fancy/palettes/night.json"
            )),
        }))
    })
}

fn embedded_manifest(skin: SkinId) -> &'static EmbeddedManifest {
    match skin {
        SkinId::Classic => embedded_classic_manifest(),
        SkinId::Fancy => embedded_fancy_manifest(),
    }
}

fn embedded_classic_manifest() -> &'static EmbeddedManifest {
    static MANIFEST: OnceLock<&'static EmbeddedManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| Box::leak(Box::new(manifest_from_json(include_str!(
        "../assets/skins/classic/manifest.json"
    )))))
}

fn embedded_fancy_manifest() -> &'static EmbeddedManifest {
    static MANIFEST: OnceLock<&'static EmbeddedManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        Box::leak(Box::new(manifest_from_json(include_str!(
            "../assets/skins/fancy/manifest.json"
        ))))
    })
}

fn embedded_descriptions() -> &'static PresetDescriptions {
    static DESCRIPTIONS: OnceLock<&'static PresetDescriptions> = OnceLock::new();
    DESCRIPTIONS.get_or_init(|| Box::leak(Box::new(descriptions_from_json())))
}

fn descriptions_from_json() -> PresetDescriptions {
    let file: SettingsDescriptionsFile =
        serde_json::from_str(include_str!("../assets/skins/settings-descriptions.json"))
            .expect("settings-descriptions.json must parse");
    PresetDescriptions {
        entries: [
            (
                "classic-day",
                LocalizedCopy {
                    en: leak_copy(file.classic_day.en),
                    zh_hant: leak_copy(file.classic_day.zh_hant),
                },
            ),
            (
                "classic-night",
                LocalizedCopy {
                    en: leak_copy(file.classic_night.en),
                    zh_hant: leak_copy(file.classic_night.zh_hant),
                },
            ),
            (
                "fancy-day",
                LocalizedCopy {
                    en: leak_copy(file.fancy_day.en),
                    zh_hant: leak_copy(file.fancy_day.zh_hant),
                },
            ),
            (
                "fancy-night",
                LocalizedCopy {
                    en: leak_copy(file.fancy_night.en),
                    zh_hant: leak_copy(file.fancy_night.zh_hant),
                },
            ),
        ],
    }
}

fn leak_copy(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn manifest_from_json(json: &str) -> EmbeddedManifest {
    let file: SkinManifestFile = serde_json::from_str(json).expect("skin manifest must parse");
    EmbeddedManifest {
        brand_short: leak_copy(file.brand_short),
        title_template: leak_copy(file.title_template),
        title_template_with_instance: leak_copy(file.title_template_with_instance),
        metrics: SkinMetrics {
            corner_radius_control_px: file.metrics.corner_radius_control_px,
            corner_radius_modal_px: file.metrics.corner_radius_modal_px,
            border_width_px: file.metrics.border_width_px,
            scrollbar_style: leak_copy(file.metrics.scrollbar_style),
        },
    }
}

fn palette_from_json(json: &str) -> ThemePalette {
    let file: PaletteFile = serde_json::from_str(json).expect("palette json must parse");
    let colors = file.colors;
    let ansi: [Rgb; 16] = colors
        .ansi
        .iter()
        .map(|value| parse_hex_color(value).expect("palette ansi hex"))
        .collect::<Vec<_>>()
        .try_into()
        .expect("palette ansi must contain 16 entries");
    ThemePalette {
        sidebar: parse_hex_color(&colors.sidebar).expect("sidebar hex"),
        terminal_background: parse_hex_color(&colors.terminal_background)
            .expect("terminal_background hex"),
        composer: parse_hex_color(&colors.composer).expect("composer hex"),
        modal: parse_hex_color(&colors.modal).expect("modal hex"),
        status: parse_hex_color(&colors.status).expect("status hex"),
        text: parse_hex_color(&colors.text).expect("text hex"),
        muted_text: parse_hex_color(&colors.muted_text).expect("muted_text hex"),
        divider: parse_hex_color(&colors.divider).expect("divider hex"),
        control: parse_hex_color(&colors.control).expect("control hex"),
        control_hover: parse_hex_color(&colors.control_hover).expect("control_hover hex"),
        control_pressed: parse_hex_color(&colors.control_pressed).expect("control_pressed hex"),
        active: parse_hex_color(&colors.active).expect("active hex"),
        active_border: parse_hex_color(&colors.active_border).expect("active_border hex"),
        focus_ring: parse_hex_color(&colors.focus_ring).expect("focus_ring hex"),
        success: parse_hex_color(&colors.success).expect("success hex"),
        warning: parse_hex_color(&colors.warning).expect("warning hex"),
        danger: parse_hex_color(&colors.danger).expect("danger hex"),
        accent: parse_hex_color(&colors.accent).expect("accent hex"),
        terminal_foreground: parse_hex_color(&colors.terminal_foreground)
            .expect("terminal_foreground hex"),
        selection_background: parse_hex_color(&colors.selection_background)
            .expect("selection_background hex"),
        selection_foreground: parse_hex_color(&colors.selection_foreground)
            .expect("selection_foreground hex"),
        scrollbar_track: parse_hex_color(&colors.scrollbar_track).expect("scrollbar_track hex"),
        scrollbar_thumb: parse_hex_color(&colors.scrollbar_thumb).expect("scrollbar_thumb hex"),
        scrollbar_thumb_active: parse_hex_color(&colors.scrollbar_thumb_active)
            .expect("scrollbar_thumb_active hex"),
        ansi,
    }
}

fn parse_hex_color(value: &str) -> Result<Rgb, String> {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("invalid hex color: {value}"));
    }
    let parsed = u32::from_str_radix(value, 16).map_err(|error| error.to_string())?;
    Ok(Rgb::new(
        ((parsed >> 16) & 0xFF) as u8,
        ((parsed >> 8) & 0xFF) as u8,
        (parsed & 0xFF) as u8,
    ))
}

pub(crate) fn window_title_for_preset(
    preset: AppearancePreset,
    version: &str,
    instance: Option<&str>,
) -> String {
    preset.window_title(version, instance)
}

pub(crate) fn decode_window_icon_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    use png::{ColorType, Decoder, Transformations};
    let mut decoder = Decoder::new(bytes);
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    let width = info.width;
    let height = info.height;
    let rgba = match info.color_type {
        ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
        ColorType::Rgb => buffer[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        _ => return None,
    };
    Some((width, height, rgba))
}

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
    fn appearance_presets_have_stable_ids_and_migration() {
        assert_eq!(
            AppearancePreset::ALL.map(AppearancePreset::as_str),
            [
                "classic-day",
                "classic-night",
                "fancy-day",
                "fancy-night"
            ]
        );
        assert_eq!(
            AppearancePreset::parse("dark"),
            AppearancePreset::classic_night()
        );
        assert_eq!(
            AppearancePreset::parse("light"),
            AppearancePreset::classic_day()
        );
        assert_eq!(
            AppearancePreset::classic_day().color_theme(),
            ThemeId::Light
        );
        assert_eq!(*AppearancePreset::classic_day().palette(), LIGHT);
        assert_eq!(*AppearancePreset::classic_night().palette(), DARK);
        assert_ne!(
            AppearancePreset::fancy_day().palette().accent,
            AppearancePreset::classic_day().palette().accent
        );
        assert_ne!(
            AppearancePreset::fancy_night().palette().accent,
            AppearancePreset::classic_night().palette().accent
        );
        assert_eq!(
            AppearancePreset::classic_day().description(LocaleId::English),
            "Industrial light chrome; rectilinear controls and today's default brightness."
        );
        assert_eq!(
            AppearancePreset::fancy_day().window_title("0.1.14", None),
            "AgenTerm · 0.1.14"
        );
        assert_eq!(
            serde_json::to_string(&AppearancePreset::fancy_day()).unwrap(),
            "\"fancy-day\""
        );
    }

    #[test]
    fn decode_window_icon_png_loads_fancy_png() {
        let (width, height, rgba) =
            decode_window_icon_png(AppearancePreset::fancy_day().window_icon_png())
                .expect("fancy icon png");
        assert!(width > 0 && height > 0);
        assert_eq!(rgba.len(), (width * height * 4) as usize);
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
            assert!(contrast(palette.text, palette.active) >= 4.5);
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
