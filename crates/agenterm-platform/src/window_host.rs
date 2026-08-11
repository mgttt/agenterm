//! Stable pixel-window facade.
//!
//! Native event-loop and surface types remain owned by the selected adapter.

pub use crate::contract::pixel_present::{
    PixelPresentOutcome, PixelPresentReceipt, PixelPresentRegion, PixelPresentStats,
};
pub use crate::contract::window_host::{
    GeometryChange, LogicalPoint, LogicalRect, LogicalSize, PixelBackingRetention, PixelFrameError,
    PixelFrameGeneration, PixelFrameInfo, PixelFrameWrite, PixelFrameWriteReceipt,
    PixelPointerCursor, PixelRect, PixelWindow, PixelWindowApplication, PixelWindowDirective,
    PixelWindowError, PixelWindowEvent, PixelWindowMetrics, PixelWindowOptions, PointerButton,
    PointerButtonState, WheelDelta, WindowSemanticFlags, WindowWaker, XrgbPixelFrame,
};

/// Run one native pixel window until the application requests exit or the
/// selected event loop terminates.
pub fn run_pixel_window(
    options: PixelWindowOptions,
    application: Box<dyn PixelWindowApplication>,
) -> Result<(), PixelWindowError> {
    if !options.initial_logical_size.is_valid() {
        return Err(PixelWindowError::failed(
            "pixel_window_invalid_initial_size",
            "initial logical width and height must be finite and greater than zero",
        ));
    }
    crate::selected::window::run_pixel_window(options, application)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    struct ApiApplication;

    impl PixelWindowApplication for ApiApplication {
        fn opened(
            &mut self,
            _window: &PixelWindow,
        ) -> Result<PixelWindowDirective, PixelWindowError> {
            Ok(PixelWindowDirective::Continue)
        }

        fn event(
            &mut self,
            _window: &PixelWindow,
            _event: PixelWindowEvent,
        ) -> Result<PixelWindowDirective, PixelWindowError> {
            Ok(PixelWindowDirective::Continue)
        }

        fn render(
            &mut self,
            _window: &PixelWindow,
            frame: &mut XrgbPixelFrame<'_>,
        ) -> Result<PixelWindowDirective, PixelWindowError> {
            frame.pixels_mut().fill(0x0012_3456);
            Ok(PixelWindowDirective::Continue)
        }

        fn about_to_wait(
            &mut self,
            _window: &PixelWindow,
            _now: Instant,
        ) -> Result<PixelWindowDirective, PixelWindowError> {
            Ok(PixelWindowDirective::Wait)
        }
    }

    #[test]
    fn public_application_contract_compiles_without_native_types() {
        fn accepts_application(_: Box<dyn PixelWindowApplication>) {}
        accepts_application(Box::new(ApiApplication));
    }

    #[test]
    fn invalid_initial_size_fails_before_native_dispatch() {
        let options = PixelWindowOptions::new("invalid", LogicalSize::new(0.0, 480.0));
        let error = run_pixel_window(options, Box::new(ApiApplication)).expect_err("invalid size");
        assert!(matches!(error, PixelWindowError::Failed { .. }));
    }

    #[cfg(all(
        windows,
        not(feature = "native-pixel-window"),
        not(feature = "portable-pixel-window")
    ))]
    #[test]
    fn windows_runner_reports_typed_unsupported() {
        let options = PixelWindowOptions::new("unsupported", LogicalSize::new(760.0, 480.0));
        let error = run_pixel_window(options, Box::new(ApiApplication)).expect_err("unsupported");
        assert!(matches!(error, PixelWindowError::Unsupported { .. }));
    }

    #[cfg(all(windows, feature = "native-pixel-window"))]
    #[test]
    fn native_windows_runner_honors_exit_from_opened_without_leaking_a_window() {
        struct ExitOnOpen;

        impl PixelWindowApplication for ExitOnOpen {
            fn opened(
                &mut self,
                _window: &PixelWindow,
            ) -> Result<PixelWindowDirective, PixelWindowError> {
                Ok(PixelWindowDirective::Exit)
            }

            fn event(
                &mut self,
                _window: &PixelWindow,
                _event: PixelWindowEvent,
            ) -> Result<PixelWindowDirective, PixelWindowError> {
                Ok(PixelWindowDirective::Exit)
            }

            fn render(
                &mut self,
                _window: &PixelWindow,
                _frame: &mut XrgbPixelFrame<'_>,
            ) -> Result<PixelWindowDirective, PixelWindowError> {
                Ok(PixelWindowDirective::Exit)
            }

            fn about_to_wait(
                &mut self,
                _window: &PixelWindow,
                _now: Instant,
            ) -> Result<PixelWindowDirective, PixelWindowError> {
                Ok(PixelWindowDirective::Exit)
            }
        }

        let options = PixelWindowOptions::new("native-test", LogicalSize::new(64.0, 64.0))
            .with_no_activate(true);
        run_pixel_window(options, Box::new(ExitOnOpen)).expect("native runner must exit cleanly");
    }
}
