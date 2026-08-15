use std::time::Instant;

use agenterm_platform::window_host::{
    LogicalSize, PixelWindow, PixelWindowApplication, PixelWindowDirective, PixelWindowError,
    PixelWindowEvent, PixelWindowOptions, XrgbPixelFrame, run_pixel_window,
};

use super::LoadedImage;

const BACKGROUND: u32 = 0x0014_1b24;
const LOADED: u32 = 0x0036_b37e;

pub(super) fn present(image: LoadedImage) -> Result<(), PixelWindowError> {
    present_with(image, run_pixel_window)
}

fn present_with<E>(
    image: LoadedImage,
    runner: impl FnOnce(PixelWindowOptions, Box<dyn PixelWindowApplication>) -> Result<(), E>,
) -> Result<(), E> {
    let title = format!("AgenTerm Chassis — {}", image.l3_name());
    let options = PixelWindowOptions::new(title, LogicalSize::new(560.0, 240.0));
    runner(options, Box::new(LoaderApplication { image }))
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

#[cfg(test)]
mod tests {
    use std::fs;

    use agenterm_chassis::CELLS;

    use super::{super::load_image, present_with};

    #[test]
    fn checked_image_reaches_native_runner_and_propagates_failure() {
        let native_entrypoint = super::super::present_image;
        let _ = native_entrypoint;

        let root = std::env::temp_dir().join(format!(
            "agenterm-native-present-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        write_valid_image(&root);
        let image = load_image(&root).expect("checked image");
        let mut calls = 0;

        let result = present_with(image, |_options, _application| {
            calls += 1;
            Err("native presenter failed")
        });

        assert_eq!(calls, 1);
        assert_eq!(result, Err("native presenter failed"));
        let _ = fs::remove_dir_all(root);
    }

    fn write_valid_image(root: &std::path::Path) {
        for cell in CELLS {
            let cell_root = root.join("l1").join(cell);
            fs::create_dir_all(&cell_root).expect("L1 cell");
            fs::write(cell_root.join("loader"), format!("frozen-{cell}")).expect("L1 loader");
        }
        fs::create_dir_all(root.join("l2")).expect("L2");
        fs::write(
            root.join("l2/host-abi.json"),
            include_str!("../../l2/host-abi.json"),
        )
        .expect("Host ABI");
        fs::create_dir_all(root.join("l3")).expect("L3");
        fs::write(
            root.join("l3/app.json"),
            serde_json::json!({
                "schema": 1,
                "name": "native.presenter.test",
                "capabilities": ["tabs.active"],
            })
            .to_string(),
        )
        .expect("L3 manifest");
        fs::write(
            root.join("manifest.json"),
            serde_json::json!({
                "schema": 1,
                "compile": false,
                "invokes_cargo": false,
                "cells": CELLS,
            })
            .to_string(),
        )
        .expect("product manifest");
    }
}
