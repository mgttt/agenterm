//! Closed action enum. Kebab CLI names and Spectacle constants are one meaning.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaceAction {
    Center,
    Fullscreen,
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    UpperLeft,
    LowerLeft,
    UpperRight,
    LowerRight,
    NextThird,
    PreviousThird,
    NextDisplay,
    PreviousDisplay,
    Larger,
    Smaller,
    Undo,
    Redo,
}

impl PlaceAction {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "center" | "SpectacleWindowActionCenter" => Some(Self::Center),
            "fullscreen" | "SpectacleWindowActionFullscreen" => Some(Self::Fullscreen),
            "left-half" | "SpectacleWindowActionLeftHalf" => Some(Self::LeftHalf),
            "right-half" | "SpectacleWindowActionRightHalf" => Some(Self::RightHalf),
            "top-half" | "SpectacleWindowActionTopHalf" => Some(Self::TopHalf),
            "bottom-half" | "SpectacleWindowActionBottomHalf" => Some(Self::BottomHalf),
            "upper-left" | "SpectacleWindowActionUpperLeft" => Some(Self::UpperLeft),
            "lower-left" | "SpectacleWindowActionLowerLeft" => Some(Self::LowerLeft),
            "upper-right" | "SpectacleWindowActionUpperRight" => Some(Self::UpperRight),
            "lower-right" | "SpectacleWindowActionLowerRight" => Some(Self::LowerRight),
            "next-third" | "SpectacleWindowActionNextThird" => Some(Self::NextThird),
            "previous-third" | "SpectacleWindowActionPreviousThird" => Some(Self::PreviousThird),
            "next-display" | "SpectacleWindowActionNextDisplay" => Some(Self::NextDisplay),
            "previous-display" | "SpectacleWindowActionPreviousDisplay" => {
                Some(Self::PreviousDisplay)
            }
            "larger" | "SpectacleWindowActionLarger" => Some(Self::Larger),
            "smaller" | "SpectacleWindowActionSmaller" => Some(Self::Smaller),
            "undo" | "SpectacleWindowActionUndo" => Some(Self::Undo),
            "redo" | "SpectacleWindowActionRedo" => Some(Self::Redo),
            _ => None,
        }
    }

    pub fn kebab(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::Fullscreen => "fullscreen",
            Self::LeftHalf => "left-half",
            Self::RightHalf => "right-half",
            Self::TopHalf => "top-half",
            Self::BottomHalf => "bottom-half",
            Self::UpperLeft => "upper-left",
            Self::LowerLeft => "lower-left",
            Self::UpperRight => "upper-right",
            Self::LowerRight => "lower-right",
            Self::NextThird => "next-third",
            Self::PreviousThird => "previous-third",
            Self::NextDisplay => "next-display",
            Self::PreviousDisplay => "previous-display",
            Self::Larger => "larger",
            Self::Smaller => "smaller",
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }

    pub fn spectacle_id(self) -> &'static str {
        match self {
            Self::Center => "SpectacleWindowActionCenter",
            Self::Fullscreen => "SpectacleWindowActionFullscreen",
            Self::LeftHalf => "SpectacleWindowActionLeftHalf",
            Self::RightHalf => "SpectacleWindowActionRightHalf",
            Self::TopHalf => "SpectacleWindowActionTopHalf",
            Self::BottomHalf => "SpectacleWindowActionBottomHalf",
            Self::UpperLeft => "SpectacleWindowActionUpperLeft",
            Self::LowerLeft => "SpectacleWindowActionLowerLeft",
            Self::UpperRight => "SpectacleWindowActionUpperRight",
            Self::LowerRight => "SpectacleWindowActionLowerRight",
            Self::NextThird => "SpectacleWindowActionNextThird",
            Self::PreviousThird => "SpectacleWindowActionPreviousThird",
            Self::NextDisplay => "SpectacleWindowActionNextDisplay",
            Self::PreviousDisplay => "SpectacleWindowActionPreviousDisplay",
            Self::Larger => "SpectacleWindowActionLarger",
            Self::Smaller => "SpectacleWindowActionSmaller",
            Self::Undo => "SpectacleWindowActionUndo",
            Self::Redo => "SpectacleWindowActionRedo",
        }
    }

    pub fn is_history(self) -> bool {
        matches!(self, Self::Undo | Self::Redo)
    }

    pub fn is_display_walk(self) -> bool {
        matches!(self, Self::NextDisplay | Self::PreviousDisplay)
    }
}

#[cfg(test)]
mod tests {
    use super::PlaceAction;

    const CATALOG: [PlaceAction; 18] = [
        PlaceAction::Center,
        PlaceAction::Fullscreen,
        PlaceAction::LeftHalf,
        PlaceAction::RightHalf,
        PlaceAction::TopHalf,
        PlaceAction::BottomHalf,
        PlaceAction::UpperLeft,
        PlaceAction::LowerLeft,
        PlaceAction::UpperRight,
        PlaceAction::LowerRight,
        PlaceAction::NextThird,
        PlaceAction::PreviousThird,
        PlaceAction::NextDisplay,
        PlaceAction::PreviousDisplay,
        PlaceAction::Larger,
        PlaceAction::Smaller,
        PlaceAction::Undo,
        PlaceAction::Redo,
    ];

    #[test]
    fn closed_catalog_accepts_exactly_both_frozen_spellings() {
        assert_eq!(CATALOG.len(), 18);
        for action in CATALOG {
            assert_eq!(PlaceAction::parse(action.kebab()), Some(action));
            assert_eq!(PlaceAction::parse(action.spectacle_id()), Some(action));
        }

        // The catalog intentionally has no compatibility alias soup.
        for invalid in [
            "",
            "left",
            "LeftHalf",
            "left_half",
            "SpectacleWindowActionTileMagic",
        ] {
            assert_eq!(PlaceAction::parse(invalid), None, "accepted {invalid:?}");
        }
    }

    #[test]
    fn history_and_display_walk_classes_are_exhaustive() {
        for action in CATALOG {
            assert_eq!(
                action.is_history(),
                matches!(action, PlaceAction::Undo | PlaceAction::Redo)
            );
            assert_eq!(
                action.is_display_walk(),
                matches!(
                    action,
                    PlaceAction::NextDisplay | PlaceAction::PreviousDisplay
                )
            );
        }
    }
}
