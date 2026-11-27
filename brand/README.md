# Fractile brand

| File | What it is |
| --- | --- |
| `fractile-icon.svg` | The app icon, complete: the dark tile, the windows of a split screen with the left one lit orange, and the F on top. Builds without Xcode rasterise this file (`scripts/make-icns.swift`). |
| `icon/windows.svg`, `icon/focus.svg`, `icon/mark.svg` | The icon's layers on their own, flat, for Icon Composer: the quiet windows, the focused window and the F. `app/AppIcon.icon` uses copies of them. |
| `fractile-mark.svg` | The F mark on its own: a slanted stem with a long top bar and a shorter middle bar, both cut at an angle. |
| `fractile-wordmark.svg` | The lowercase wordmark, `fractile`, set in a bold geometric sans. It is live text: outline it in a design tool before printing it or sending it to anyone. |
| `fractile-lockup.svg` | The mark beside the wordmark, as on the sidebar. |

The mark here was traced from a screenshot of the original logo. Swap in the original vector when
it is available: its path goes in `fractile-mark.svg`, `fractile-icon.svg`, `icon/mark.svg` (and
its copy in `app/AppIcon.icon/Assets`) and `agent/assets/mark.svg` (the in-app mark).

## The icon

Fractile splits a screen into windows; the icon shows one, split a half, a quarter and two
eighths, with the half behind the F lit orange as the focused window. The F stays whole so it
reads at 16 pixels, where the quiet windows drop away and leave the F on its orange half.

The SVG draws its soft shadows as faint stepped copies, not blur filters, because macOS draws SVG
without filters and builds rasterise it with macOS.

## Colours

| Name | Hex | Use |
| --- | --- | --- |
| Ink | `#111111` | The mark and wordmark on light grounds |
| Paper | `#EEECE6` | The F on the icon, the mark on dark grounds |
| Tile | `#26262C` to `#0C0C0F` | The icon's ground, top to bottom |
| Focus | `#FF7A48` to `#F2471C` | The focused window, top to bottom |

## Where it shows

- The app icon (`app/AppIcon.icon`): with the full Xcode app, `actool` compiles the layers with
  Liquid Glass. Without Xcode, `scripts/make-icns.swift` rasterises `fractile-icon.svg` and packs
  it with `iconutil`.
- Settings: the lockup above the version at the foot of the sidebar (`agent/src/brand.rs`).
- The setup tour's welcome step: the icon, drawn from the same measurements.
