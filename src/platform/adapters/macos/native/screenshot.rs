//! macOS screenshot encoding is supplied by `agenterm-platform`.
//!
//! This product-native module intentionally contains no AppKit/CoreGraphics
//! window handle API; AgenTerm passes its rendered XRGB framebuffer through the
//! platform crate's neutral screenshot facade.
