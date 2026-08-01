//! Shared native-window lifecycle and client-size semantics.

pub(crate) const MIN_CLIENT_WIDTH: u32 = 320;
pub(crate) const MIN_CLIENT_HEIGHT: u32 = 240;
pub(crate) const MAX_CLIENT_EXTENT: u32 = i32::MAX as u32;

pub(crate) use agenterm_platform::window::WindowSemanticState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClientSize {
    pub width: u32,
    pub height: u32,
}

impl ClientSize {
    pub(crate) fn parse(
        width: Option<&str>,
        height: Option<&str>,
    ) -> Result<Self, ClientSizeError> {
        let width = width
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(ClientSizeError::InvalidWidth)?;
        let height = height
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or(ClientSizeError::InvalidHeight)?;
        Self::new(width, height)
    }

    pub(crate) const fn new(width: u32, height: u32) -> Result<Self, ClientSizeError> {
        if width < MIN_CLIENT_WIDTH {
            return Err(ClientSizeError::InvalidWidth);
        }
        if height < MIN_CLIENT_HEIGHT {
            return Err(ClientSizeError::InvalidHeight);
        }
        if width > MAX_CLIENT_EXTENT || height > MAX_CLIENT_EXTENT {
            return Err(ClientSizeError::ExtentTooLarge);
        }
        Ok(Self { width, height })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClientSizeError {
    InvalidWidth,
    InvalidHeight,
    ExtentTooLarge,
}

impl ClientSizeError {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::InvalidWidth => "window_width_invalid",
            Self::InvalidHeight => "window_height_invalid",
            Self::ExtentTooLarge => "window_extent_too_large",
        }
    }

    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::InvalidWidth => "window-resize requires --width of at least 320",
            Self::InvalidHeight => "window-resize requires --height of at least 240",
            Self::ExtentTooLarge => "window-resize dimensions exceed the native platform limit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_size_validation_is_shared_and_bounded() {
        assert_eq!(
            ClientSize::parse(Some("320"), Some("240")),
            Ok(ClientSize {
                width: 320,
                height: 240,
            })
        );
        assert_eq!(
            ClientSize::parse(Some("319"), Some("600")),
            Err(ClientSizeError::InvalidWidth)
        );
        assert_eq!(
            ClientSize::parse(Some("640"), Some("239")),
            Err(ClientSizeError::InvalidHeight)
        );
        assert_eq!(
            ClientSize::new(MAX_CLIENT_EXTENT + 1, 600),
            Err(ClientSizeError::ExtentTooLarge)
        );
        assert_eq!(
            ClientSizeError::ExtentTooLarge.code(),
            "window_extent_too_large"
        );
    }
}
