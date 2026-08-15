use std::time::Instant;

use agenterm_platform::window_host::{
    LogicalSize, PixelWindow, PixelWindowApplication, PixelWindowDirective, PixelWindowError,
    PixelWindowEvent, PixelWindowOptions, XrgbPixelFrame, run_pixel_window,
};

use super::LoadedImage;

const BACKGROUND: u32 = 0x0014_1b24;
const LOADED: u32 = 0x0036_b37e;

pub(super) fn present(image: LoadedImage) -> Result<(), PixelWindowError> {
    let title = format!("AgenTerm Chassis — {}", image.l3_name());
    let options = PixelWindowOptions::new(title, LogicalSize::new(560.0, 240.0));
    run_pixel_window(options, Box::new(LoaderApplication { image }))
}

struct LoaderApplication {
    // Keeping the checked image here makes the loaded state resident for the
    // complete native-window lifetime.
    image: LoadedImage,
}

impl PixelWindowApplication for LoaderApplication {
    fn opened(&mut self, window: &PixelWindow) -> Result<PixelWindowDirective, PixelWindowError> {
        window.set_visible(true);
        window.request_redraw();
        Ok(PixelWindowDirective::Continue)
    }

    fn event(
        &mut self,
        _window: &PixelWindow,
        event: PixelWindowEvent,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        if matches!(event, PixelWindowEvent::CloseRequested) {
            Ok(PixelWindowDirective::Exit)
        } else {
            Ok(PixelWindowDirective::Continue)
        }
    }

    fn render(
        &mut self,
        _window: &PixelWindow,
        frame: &mut XrgbPixelFrame<'_>,
    ) -> Result<PixelWindowDirective, PixelWindowError> {
        let width = frame.width() as usize;
        let loaded_rows = 6usize.saturating_add(self.image.capability_count().min(10));
        let loaded_pixels = width.saturating_mul(loaded_rows).min(frame.pixels().len());
        let pixels = frame.pixels_mut();
        pixels.fill(BACKGROUND);
        pixels[..loaded_pixels].fill(LOADED);
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
