//! Server-strip chrome: layout, labels, dialogs, and context menu.
//!
//! Product intent:
//! - Chips are **equal width**, packed **left-to-right** (no last-chip stretch).
//! - Labels use a short instance name, centered; state is color not long text.
//! - Trailing `[+]` stays right-aligned in the strip gutter.
//! - Context menu is left-aligned under the chip, with consistent item padding.

/// How often a host may re-scan the instance registry to refresh chips.
///
/// Shared so the two frontends cannot drift into different refresh rates.
pub(crate) const SERVER_TABS_REFRESH: std::time::Duration = std::time::Duration::from_secs(2);

/// Preferred equal chip width when the strip has room.
pub(crate) const SERVER_TAB_PREFERRED_WIDTH: i32 = 112;
/// Floor so a short name like `main` still has a comfortable hit target.
pub(crate) const SERVER_TAB_MIN_WIDTH: i32 = 72;
/// Cap so one noisy label cannot dominate the strip.
pub(crate) const SERVER_TAB_MAX_WIDTH: i32 = 140;
pub(crate) const SERVER_TAB_GAP: i32 = 6;
pub(crate) const SERVER_TAB_STRIP_INSET: i32 = 6;
pub(crate) const SERVER_TAB_CHIP_V_INSET: i32 = 4;
pub(crate) const SERVER_ADD_WIDTH: i32 = 28;
pub(crate) const SERVER_CONTEXT_MENU_WIDTH: i32 = 156;
pub(crate) const SERVER_CONTEXT_MENU_ITEM_HEIGHT: i32 = 30;
pub(crate) const SERVER_CONTEXT_MENU_PAD: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StripRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl StripRect {
    // Production callers work in host pixel-rect types after the
    // ServerContextMenuRects::map conversion; only unit tests assert raw
    // strip geometry through this accessor today.
    #[cfg_attr(not(test), expect(dead_code, reason = "test-only geometry accessor"))]
    pub(crate) const fn width(self) -> i32 {
        self.right - self.left
    }

    #[allow(dead_code)] // handy for hit-tests / future layout asserts
    pub(crate) const fn height(self) -> i32 {
        self.bottom - self.top
    }

    #[allow(dead_code)] // handy for hit-tests / future layout asserts
    pub(crate) const fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// Short chip label: instance token only (state is conveyed by fill color).
pub(crate) fn server_tab_chip_label(instance: &str, can_attach: bool) -> String {
    let short = instance.strip_prefix("custom:").unwrap_or(instance);
    if can_attach {
        short.to_owned()
    } else {
        format!("{short} · stale")
    }
}

/// Equal-width chips packed from the left; `[+]` is not part of this list.
pub(crate) fn layout_server_tab_chips(strip: StripRect, count: usize) -> Vec<StripRect> {
    if count == 0 {
        return Vec::new();
    }
    let count_i = count as i32;
    let gaps = SERVER_TAB_GAP * (count_i - 1).max(0);
    let chips_right = (strip.right - SERVER_TAB_STRIP_INSET - SERVER_ADD_WIDTH)
        .max(strip.left + SERVER_TAB_STRIP_INSET);
    let available = (chips_right - (strip.left + SERVER_TAB_STRIP_INSET) - gaps).max(count_i);
    let equal = available / count_i;
    let width = equal
        .clamp(1, SERVER_TAB_MAX_WIDTH)
        .min(SERVER_TAB_PREFERRED_WIDTH)
        .max(SERVER_TAB_MIN_WIDTH.min(equal));
    // If still wider than available/count after prefer, shrink equally so all fit.
    let width = width.min(equal.max(1));
    let mut left = strip.left + SERVER_TAB_STRIP_INSET;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let right = left + width;
        out.push(StripRect {
            left,
            top: strip.top + SERVER_TAB_CHIP_V_INSET,
            right,
            bottom: strip.bottom - SERVER_TAB_CHIP_V_INSET,
        });
        left = right + SERVER_TAB_GAP;
    }
    out
}

pub(crate) fn layout_server_add_chip(strip: StripRect) -> StripRect {
    StripRect {
        left: strip.right - SERVER_TAB_STRIP_INSET - SERVER_ADD_WIDTH,
        top: strip.top + SERVER_TAB_CHIP_V_INSET,
        right: strip.right - SERVER_TAB_STRIP_INSET,
        bottom: strip.bottom - SERVER_TAB_CHIP_V_INSET,
    }
}

/// Context-menu rectangles by name, generic over the host's rect type.
///
/// Was a positional 3-tuple, and the two hosts disagreed on the order:
/// unix passed through `(frame, as_window, close)` while windows re-packed
/// as `(frame, close, as_window)` — every consumer compensated, so behavior
/// matched only by convention, and one reordering away from a silent swap
/// of the `As Window`/`Close` hit targets. Named fields end the convention.
pub(crate) struct ServerContextMenuRects<R> {
    pub(crate) frame: R,
    pub(crate) as_window: R,
    pub(crate) close: R,
}

impl<R> ServerContextMenuRects<R> {
    /// Convert every rect with `f` — hosts use this to go from `StripRect`
    /// to their own pixel-rect type without touching field pairing.
    pub(crate) fn map<T>(self, mut f: impl FnMut(R) -> T) -> ServerContextMenuRects<T> {
        ServerContextMenuRects {
            frame: f(self.frame),
            as_window: f(self.as_window),
            close: f(self.close),
        }
    }
}

/// Menu frame + `as_window`/`close` items, aligned under `anchor_left`.
pub(crate) fn layout_server_context_menu(
    origin_x: i32,
    origin_y: i32,
    client_right: i32,
    client_bottom: i32,
    anchor_left: Option<i32>,
) -> ServerContextMenuRects<StripRect> {
    let width = SERVER_CONTEXT_MENU_WIDTH;
    let item_h = SERVER_CONTEXT_MENU_ITEM_HEIGHT;
    let pad = SERVER_CONTEXT_MENU_PAD;
    let height = item_h * 2 + pad * 2;
    let preferred_left = anchor_left.unwrap_or(origin_x);
    let left = preferred_left.clamp(0, (client_right - width).max(0));
    let top = origin_y.clamp(0, (client_bottom - height).max(0));
    let frame = StripRect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    let as_window = StripRect {
        left: left + pad,
        top: top + pad,
        right: left + width - pad,
        bottom: top + pad + item_h,
    };
    let close = StripRect {
        left: left + pad,
        top: top + pad + item_h,
        right: left + width - pad,
        bottom: top + pad + item_h * 2,
    };
    ServerContextMenuRects {
        frame,
        as_window,
        close,
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ServerNewDialog {
    open: bool,
    name: String,
    error: Option<String>,
}

impl ServerNewDialog {
    pub(crate) const fn new() -> Self {
        Self {
            open: false,
            name: String::new(),
            error: None,
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn open(&mut self) {
        self.open = true;
        self.name.clear();
        self.error = None;
    }

    pub(crate) fn close(&mut self) {
        self.open = false;
        self.name.clear();
        self.error = None;
    }

    pub(crate) fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// Normalize user input to a bare instance token (no `custom:` prefix).
    pub(crate) fn take_validated_name(&mut self) -> Result<String, String> {
        let raw = self.name.trim();
        if raw.is_empty() {
            return Err("server name is required".to_owned());
        }
        let bare = raw.strip_prefix("custom:").unwrap_or(raw).trim();
        if bare.is_empty() {
            return Err("server name is required".to_owned());
        }
        if bare.eq_ignore_ascii_case("main") {
            return Err("use a custom name; `main` is the default instance".to_owned());
        }
        if bare.len() > 48 {
            return Err("server name is too long (max 48)".to_owned());
        }
        if !bare
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return Err("use letters, digits, '-' or '_' only".to_owned());
        }
        Ok(bare.to_owned())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ServerTabContextMenu {
    pub instance: String,
    pub endpoint: String,
    pub can_attach: bool,
    /// Menu top-left in client coordinates.
    pub origin_x: i32,
    pub origin_y: i32,
}

#[derive(Clone, Debug)]
pub(crate) struct ServerCloseConfirm {
    pub instance: String,
    pub endpoint: String,
    pub can_attach: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerContextAction {
    Close,
    NewWindow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_are_equal_width_and_left_packed() {
        let strip = StripRect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 32,
        };
        let chips = layout_server_tab_chips(strip, 3);
        assert_eq!(chips.len(), 3);
        let width = chips[0].width();
        assert!(width >= SERVER_TAB_MIN_WIDTH);
        assert!(width <= SERVER_TAB_PREFERRED_WIDTH);
        assert_eq!(chips[1].width(), width);
        assert_eq!(chips[2].width(), width);
        assert_eq!(chips[0].left, SERVER_TAB_STRIP_INSET);
        assert_eq!(chips[1].left, chips[0].right + SERVER_TAB_GAP);
        // Last chip must not stretch to the [+] gutter.
        let add = layout_server_add_chip(strip);
        assert!(chips[2].right + SERVER_TAB_GAP <= add.left);
    }

    #[test]
    fn many_chips_shrink_equally_instead_of_dropping() {
        let strip = StripRect {
            left: 0,
            top: 0,
            right: 400,
            bottom: 32,
        };
        let chips = layout_server_tab_chips(strip, 8);
        assert_eq!(chips.len(), 8);
        let width = chips[0].width();
        assert!(chips.iter().all(|chip| chip.width() == width));
        assert!(width >= 1);
        let add = layout_server_add_chip(strip);
        assert!(chips.last().unwrap().right <= add.left);
    }

    #[test]
    fn chip_label_prefers_short_instance_token() {
        assert_eq!(server_tab_chip_label("main", true), "main");
        assert_eq!(server_tab_chip_label("custom:work", true), "work");
        assert_eq!(server_tab_chip_label("custom:work", false), "work · stale");
    }

    #[test]
    fn context_menu_aligns_under_chip_and_orders_as_window_then_close() {
        let menu = layout_server_context_menu(100, 40, 1000, 800, Some(120));
        assert_eq!(menu.frame.left, 120);
        assert!(menu.as_window.top < menu.close.top);
        assert_eq!(menu.as_window.left, menu.close.left);
        assert_eq!(menu.as_window.width(), menu.close.width());
    }
}
