use crate::window::DisplayBackendFacts;
pub(crate) fn display_backend_facts() -> DisplayBackendFacts {
    DisplayBackendFacts {
        x11: false,
        wayland: false,
        headless: false,
    }
}
