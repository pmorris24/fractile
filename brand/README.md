# Fractile brand

| File | What it is |
| --- | --- |
| `fractile-mark.svg` | The F mark on its own: a slanted stem with a long top bar and a shorter middle bar, both cut at an angle. |
| `fractile-wordmark.svg` | The lowercase wordmark, `fractile`, set in a bold geometric sans. It is live text: outline it in a design tool before printing it or sending it to anyone. |
| `fractile-lockup.svg` | The mark beside the wordmark, as on the sidebar. |

The mark here was traced from a screenshot of the original logo. Swap in the original vector when
it is available, then copy its paths into `agent/assets/mark.svg` (the in-app mark),
`scripts/generate-app-icon.swift` (which writes `app/AppIcon.icon/Assets/Mark.svg`) and
`scripts/make-icns.swift` (the icon built without Xcode).

## Colours

| Name | Hex | Use |
| --- | --- | --- |
| Ink | `#111111` | The mark and wordmark |
| Paper | `#EEECE6` | Behind the mark: the app icon and the setup tour's badge |

## Where it shows

- The app icon (`app/AppIcon.icon`): black mark on paper. With the full Xcode app, `actool`
  compiles it. Without Xcode, `scripts/make-icns.swift` draws the same icon and packs it with
  `iconutil`, so the Dock and Finder still show it.
- Settings: the lockup above the version at the foot of the sidebar (`agent/src/brand.rs`).
- The setup tour's welcome step: the mark on its paper badge.
