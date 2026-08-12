//! xterm 256-color palette and `vt100::Color` resolution.
//!
//! Generated programmatically from the standard xterm definition so the
//! table cannot drift from what TUI applications emit.

/// An RGB triple in the frame's XRGB `0x00RRGGBB` pixel layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    #[inline]
    pub const fn to_xrgb(self) -> u32 {
        // XRGB layout 0x00RRGGBB; the top byte is left zero.
        (self.0 as u32) << 16 | (self.1 as u32) << 8 | (self.2 as u32)
    }
}

/// Mixes `from` toward `to` by `amount` (0.0 = `from`, 1.0 = `to`).
///
/// Used for the dim attribute, which is expressed as a colour blended toward
/// the background rather than a separate palette entry.
#[inline]
pub fn blend(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    let amount = clamp_f32(amount, 0.0, 1.0);
    let mix = |a: u8, b: u8| {
        let a = f32::from(a);
        let b = f32::from(b);
        clamp_f32(
            agenterm_platform::numeric::round_f32(a + (b - a) * amount),
            0.0,
            255.0,
        ) as u8
    };
    Rgb(mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2))
}

#[allow(clippy::manual_clamp)] // Float bounds are ordered constants; avoid fmt panic glue.
fn clamp_f32(value: f32, minimum: f32, maximum: f32) -> f32 {
    if value < minimum {
        minimum
    } else if value > maximum {
        maximum
    } else {
        value
    }
}

/// Builds the standard 256-entry xterm palette once.
fn palette() -> &'static [Rgb; 256] {
    use std::sync::OnceLock;
    static PALETTE: OnceLock<Box<[Rgb; 256]>> = OnceLock::new();
    PALETTE.get_or_init(|| {
        let mut table = Box::new([Rgb(0, 0, 0); 256]);

        // 0..16: ANSI standard + bright. Matches xterm's built-in set.
        let ansi: [[u8; 3]; 16] = [
            [0x00, 0x00, 0x00],
            [0xCD, 0x00, 0x00],
            [0x00, 0xCD, 0x00],
            [0xCD, 0xCD, 0x00],
            [0x00, 0x00, 0xEE],
            [0xCD, 0x00, 0xCD],
            [0x00, 0xCD, 0xCD],
            [0xE5, 0xE5, 0xE5],
            [0x7F, 0x7F, 0x7F],
            [0xFF, 0x00, 0x00],
            [0x00, 0xFF, 0x00],
            [0xFF, 0xFF, 0x00],
            [0x5C, 0x5C, 0xFF],
            [0xFF, 0x00, 0xFF],
            [0x00, 0xFF, 0xFF],
            [0xFF, 0xFF, 0xFF],
        ];
        for (index, rgb) in ansi.iter().enumerate() {
            table[index] = Rgb(rgb[0], rgb[1], rgb[2]);
        }

        // 16..232: 6x6x6 color cube. Component levels follow the xterm ramp.
        let levels = [0u8, 95, 135, 175, 215, 255];
        let mut index = 16;
        for r in 0..6 {
            for g in 0..6 {
                for b in 0..6 {
                    table[index] = Rgb(levels[r], levels[g], levels[b]);
                    index += 1;
                }
            }
        }

        // 232..256: 24-step grayscale ramp from 8 to 238.
        for step in 0u8..24 {
            let value = 8 + step * 10;
            table[232 + usize::from(step)] = Rgb(value, value, value);
        }

        table
    })
}

/// Resolves a `vt100::Color` against the palette and application defaults.
///
/// `bold` selects the bright variant for indexed foreground colors 0..7,
/// matching the conventional "bold → bright" behavior TUI apps rely on.
#[inline]
pub fn resolve(color: vt100::Color, default: Rgb, bold: bool) -> Rgb {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Rgb(r, g, b),
        vt100::Color::Idx(index) => {
            // Bold maps the standard 0..7 range to the bright 8..15 counterparts.
            if bold && index < 8 {
                palette()[usize::from(index) + 8]
            } else {
                palette()[usize::from(index)]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_and_white_corners_are_stable() {
        assert_eq!(
            resolve(vt100::Color::Idx(16), Rgb(0, 0, 0), false),
            Rgb(0, 0, 0)
        );
        assert_eq!(
            resolve(vt100::Color::Idx(231), Rgb(0, 0, 0), false),
            Rgb(255, 255, 255)
        );
    }

    #[test]
    fn bold_promotes_standard_red_to_bright() {
        let normal = resolve(vt100::Color::Idx(1), Rgb(0, 0, 0), false);
        let bright = resolve(vt100::Color::Idx(1), Rgb(0, 0, 0), true);
        assert_eq!(normal, Rgb(0xCD, 0x00, 0x00));
        assert_eq!(bright, Rgb(0xFF, 0x00, 0x00));
    }

    #[test]
    fn rgb_passes_through_unchanged() {
        assert_eq!(
            resolve(vt100::Color::Rgb(1, 2, 3), Rgb(9, 9, 9), false),
            Rgb(1, 2, 3)
        );
    }

    #[test]
    fn xrgb_pixel_packs_channels_correctly() {
        assert_eq!(Rgb(0xFF, 0x00, 0x00).to_xrgb(), 0x00FF_0000);
        assert_eq!(Rgb(0x12, 0x34, 0x56).to_xrgb(), 0x0012_3456);
    }
}
