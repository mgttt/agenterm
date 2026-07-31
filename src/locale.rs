use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum LocaleId {
    #[default]
    English,
    TraditionalChinese,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiText {
    Add,
    Apply,
    Cancel,
    Close,
    ColorTheme,
    Copy,
    Create,
    CurrentTerminal,
    Default,
    DefaultValues,
    FontFamily,
    InheritDefault,
    Input,
    KeepServerRunning,
    Light,
    New,
    NewTerminal,
    Override,
    Paste,
    Preview,
    ResetOverrides,
    Save,
    Selected,
    Send,
    Settings,
    ShellProfile,
    Size,
    StopServerAndExit,
    Tabs,
    TerminateAndClose,
    ThemeDark,
    ToggleTabs,
}

impl LocaleId {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::English, Self::TraditionalChinese];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::TraditionalChinese => "zh-Hant",
        }
    }

    pub(crate) const fn toolbar_label(self) -> &'static str {
        match self {
            Self::English => "En|Zh",
            Self::TraditionalChinese => "Zh|En",
        }
    }

    pub(crate) const fn toggled(self) -> Self {
        match self {
            Self::English => Self::TraditionalChinese,
            Self::TraditionalChinese => Self::English,
        }
    }

    pub(crate) const fn text(self, key: UiText) -> &'static str {
        match self {
            Self::English => match key {
                UiText::Add => "Add",
                UiText::Apply => "Apply",
                UiText::Cancel => "Cancel",
                UiText::Close => "Close",
                UiText::ColorTheme => "Color theme",
                UiText::Copy => "Copy",
                UiText::Create => "Create",
                UiText::CurrentTerminal => "Current terminal",
                UiText::Default => "Default",
                UiText::DefaultValues => "Default values",
                UiText::FontFamily => "Terminal font family",
                UiText::InheritDefault => "Inherit default",
                UiText::Input => "Input",
                UiText::KeepServerRunning => "Keep Server Running",
                UiText::Light => "Light",
                UiText::New => "New",
                UiText::NewTerminal => "New terminal",
                UiText::Override => "Override",
                UiText::Paste => "Paste",
                UiText::Preview => "Preview",
                UiText::ResetOverrides => "Reset overrides",
                UiText::Save => "Save",
                UiText::Selected => "Selected",
                UiText::Send => "Send",
                UiText::Settings => "Settings",
                UiText::ShellProfile => "Shell profile",
                UiText::Size => "Size",
                UiText::StopServerAndExit => "Stop Server & Exit",
                UiText::Tabs => "Tabs",
                UiText::TerminateAndClose => "Terminate & Close",
                UiText::ThemeDark => "Dark",
                UiText::ToggleTabs => "Toggle Tabs",
            },
            Self::TraditionalChinese => match key {
                UiText::Add => "新增",
                UiText::Apply => "套用",
                UiText::Cancel => "取消",
                UiText::Close => "關閉",
                UiText::ColorTheme => "色彩主題",
                UiText::Copy => "複製",
                UiText::Create => "建立",
                UiText::CurrentTerminal => "目前終端",
                UiText::Default => "預設",
                UiText::DefaultValues => "預設值",
                UiText::FontFamily => "終端字型",
                UiText::InheritDefault => "繼承預設",
                UiText::Input => "輸入",
                UiText::KeepServerRunning => "保留伺服器執行",
                UiText::Light => "淺色",
                UiText::New => "新增",
                UiText::NewTerminal => "新增終端",
                UiText::Override => "覆寫",
                UiText::Paste => "貼上",
                UiText::Preview => "預覽",
                UiText::ResetOverrides => "重設覆寫",
                UiText::Save => "儲存",
                UiText::Selected => "已選",
                UiText::Send => "發送",
                UiText::Settings => "設定",
                UiText::ShellProfile => "Shell 設定檔",
                UiText::Size => "大小",
                UiText::StopServerAndExit => "停止伺服器並結束",
                UiText::Tabs => "標籤",
                UiText::TerminateAndClose => "終止並關閉",
                UiText::ThemeDark => "深色",
                UiText::ToggleTabs => "切換標籤區",
            },
        }
    }
}

impl Serialize for LocaleId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LocaleId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct LocaleIdVisitor;

        impl Visitor<'_> for LocaleIdVisitor {
            type Value = LocaleId;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a built-in locale ID string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(match value {
                    "zh-Hant" => LocaleId::TraditionalChinese,
                    _ => LocaleId::English,
                })
            }
        }

        deserializer.deserialize_str(LocaleIdVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_ids_are_stable_and_unknown_values_fall_back_to_english() {
        assert_eq!(
            serde_json::to_string(&LocaleId::TraditionalChinese).unwrap(),
            r#""zh-Hant""#
        );
        assert_eq!(
            serde_json::from_str::<LocaleId>(r#""future""#).unwrap(),
            LocaleId::English
        );
        assert_eq!(LocaleId::English.toggled().toggled(), LocaleId::English);
    }

    #[test]
    fn primary_controls_have_nonempty_labels_in_every_locale() {
        for locale in LocaleId::ALL {
            for key in [
                UiText::New,
                UiText::Tabs,
                UiText::Settings,
                UiText::Send,
                UiText::Apply,
                UiText::Cancel,
            ] {
                assert!(!locale.text(key).is_empty());
            }
        }
    }
}
