use crate::window::DisplayBackendFacts;
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    DisplayBackendFacts {
        x11,
        wayland,
        headless: !(x11 || wayland),
    }
}
