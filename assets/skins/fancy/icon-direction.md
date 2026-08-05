# Fancy skin icon direction (v1 placeholder)

Status: **placeholder shipped** — final art can replace `icon.png` / `icon.ico` / `icon.icns`
without changing manifest paths.

## Design brief

- **Base identity**: keep AgenTerm's terminal-chevron + stacked-panes silhouette from
  `assets/agenterm-icon.png`; do not invent a second product mark.
- **Fancy differentiation**: add a subtle teal (`#38B4C8` night / `#008298` day) inner
  glow or accent stroke on the chevron; optional 4 px rounded tile corners matching
  `corner_radius_control_px`.
- **Tone**: industrial and credible — flat vector, no gradients that blur at 16 px, no
  neon or toy-game aesthetics.
- **Avoid**: resemblance to PowerShell, Windows Terminal, VS Code, Docker, or other
  existing brands (same constraint as the classic icon README).

## Deliverables for final art

| Asset | Sizes | Notes |
|-------|-------|-------|
| `icon.png` | 256×256 transparent | Source for review and Linux runtime window icon |
| `icon.ico` | 16/20/24/32/40/48/64/128/256 | Windows embed via `build.rs` (engineering Phase 2B) |
| `icon.icns` | macOS bundle ladder | Same silhouette at all densities |

## Placeholder

`icon.png` is a generated stand-in: navy tile, classic chevron geometry, teal accent
stroke. Replace before public release if brand review requires final pixels.
