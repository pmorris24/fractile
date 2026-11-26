# Fractile Design System

Fractile's Settings window, its modals and its shortcut keys are drawn with
[Rune](https://github.com/anomalyco/rune) in OpenCode's design language, the same as Hex's wherever
both apps have the same object:

- **`rune-kit`** provides the generic components (`Button`, `Switch`, `SegmentedControl`, and
  the rest).
- **`rune-theme-opencode`** provides every colour, elevation, radius, type style and motion token
  (`rune_theme_opencode::kit(dark)`). Settings follows the system appearance, so every view works
  in light and dark.
- **[`agent/src/controls.rs`](../agent/src/controls.rs)** and `controls/` hold Fractile's own
  pieces and its layout tokens ([`controls/tokens.rs`](../agent/src/controls/tokens.rs)).

The window follows OpenCode Desktop's layout: a deep-background sidebar and a raised, rounded
content panel. It opens at 920 × 660, resizes down to 860 × 580, and its cards stop growing at 720
and centre in a wider panel.

Three surfaces draw objects, not settings rows, and keep their own measurements: the Workspace HUD
(`agent/src/hud/`), the setup tour (`agent/src/onboarding/`) and the Layout pane's desk miniature
(`agent/src/panes/layout/preview.rs`).

## Rules

1. **Kit tokens only.** Views name colours, type, radii, elevation and motion through `Paint`:
   the role helpers below, or `p.k.c.*`, `p.k.radius.*`, `p.k.elevation.*` and `p.k.motion.*`.
   Never write a colour literal outside the theme crate.
2. **One text rail, one control rail.** The pane title, section labels and row text all start
   `METRICS.gutter` (32) from the panel's edge, and every row's control ends 32 from the other
   edge. Cards hang outward by `METRICS.row_px` (16), so a card's edge is 16 from the panel's.
   Nothing indents past the rail: name a sub-row ("Top edge") instead of indenting it.
3. **Sizes come from `METRICS`, `WINDOW`, `MODAL` and `KEYS`; a new number is a new token with
   a name.** Row heights are tokens (`controls::row_frame`), never hand-typed vertical padding.
4. **A modal is a small page.** Every modal is `modal_panel` inside `modal_sheet`, with a
   `ModalWidth`, and reuses the page's rails.
5. **One accent per screen.** `ButtonKind::Primary` marks at most one action per screen. A state
   that wants attention is a Secondary button with a `status_dot`.
6. **A settings row never paints a hover.** Only the controls inside it react; a row pressed as a
   whole is built on `controls::row_frame` with its own `on_press`.
7. **Verify by looking.** Render every view you touched as a `--snap` scene at 860 × 580 and
   920 × 660, in dark and light (see [Verification](#verification)), and look at each image.

## Spatial scale (4 pt base)

Allowed steps: **0, 1, 2, 4, 6, 8, 12, 16, 20, 24, 32, 40, 48**. Anything else is a derived
concentric value with a name.

| Token | Value | Use |
|---|---:|---|
| `METRICS.gutter` | 32 | Text rail: pane title, section labels, row text, modal title and footer |
| `METRICS.row_px` | 16 | Row inset inside a card; `METRICS.card_edge()` = `gutter - row_px` = 16 |
| `METRICS.section_gap` | 24 | Between sections on a page |
| `METRICS.label_gap` | 8 | Section label to its card |
| `METRICS.header_h` | 52 | Pane header |
| `METRICS.row_h` | 48 | One-line setting row |
| `METRICS.row_h_described` | 60 | Title + description row |
| `METRICS.row_h_compact` | 28 | Sidebar item, keycap tray, small control |
| `METRICS.icon` / `icon_lead` | 16 / 28 | A row's leading glyph and the column it takes |
| `METRICS.dot` | 8 | Status dot |
| `METRICS.sidebar_w` | 220 | Sidebar |
| `METRICS.content_max` | 720 | Widest a card gets |
| `WINDOW.size` / `min` | 920 × 660 / 860 × 580 | The window as it opens, and its smallest |
| `WINDOW.panel_inset` | 8 | Content panel from the window's top, right and bottom |
| `ModalWidth::{Narrow, Medium, Wide}` | 460 / 560 / 640 | Medium: Workspace HUD |
| `MODAL.{top, bottom, gap, margin}` | 20 / 16 / 16 / 24 | A modal's vertical rhythm and its clearance from the panel |

Cards are 600 wide at the smallest window, 660 as it opens and 720 from 980 up
(`WINDOW.card_w`, asserted at compile time). The Rules map takes its width from the card, so it
never needs a number of its own.

## Concentric radius system

`R_inner = R_outer - inset` whenever the inner shape sits closer to the corner than `R_outer`.
When `inset >= R_outer` the inner shape is free and takes its component's own token.

| Shape | Radius | Token | Proof |
|---|---:|---|---|
| Sidebar item, map chip | 4 | `radius.sm` | |
| Push button, quiet button, map tile's app | 6 | `radius.md` | |
| Card, content panel, map tile | 8 | `radius.lg` | Sits 16 inside the panel: free |
| A card's first and last row | 8 | the card's own corners (`card`) | The row fills the card edge to edge |
| Modal | 10 | `radius.xl` | Cards sit 16 inside: free, stay 8 |
| Key face | 4 | `KEYS.face_radius` | |
| Key cap, rim 0.5 | 4.5 | `KEYS.cap_radius()` | `4 + 0.5` |
| Key tray, pad 3 | 7.5 | `KEYS.tray_radius()` | `4.5 + 3` (compile-time assert) |
| Recorder's hover and listening fill, pad 2 | 9.5 | `KEYS.button_radius()` | `7.5 + 2` (compile-time assert) |
| HUD marker in its capsule, inset 7 | 15 | `hud::MARKER_RADIUS` | `22 - 7` (compile-time assert) |
| Dot, switch, HUD capsule | `h / 2` | pill | |

## Typography by role

Three sizes: 14, 13, 12 (and 11 for meta). Make hierarchy by stepping size or colour, never with
two styles of the same size side by side. Views name a role, never `p.k.ty.*`.

| Role | Helper | Colour |
|---|---|---|
| Pane title, modal title | `p.title()` (14/500/21) | `p.text()` |
| Section label above a card | `p.section_label(title)` (`p.label()`, 13/530/16; a level-2 heading) | `p.muted()` |
| Row title, sidebar item | `p.row_title()` (13/440/20) | `p.text()` / `p.muted()` |
| Button, a value that matters | `p.strong()` (13/530/20) | `p.text()` |
| Row description, caption | `p.description()` (12/440/16) | `p.muted()`; `p.warning()` for a missing permission |
| Label accessory, sidebar foot, readout | `p.meta()` (11/440/12) | `p.muted()` |
| Tag, small heading | `p.tag()` (11/530/12) | `p.faint()` |
| A number that changes in place or stacks in a column | `p.tabular(style)` (`'tnum' 1`) | as its role |

## Fractile pieces

| Job | Piece |
|---|---|
| Settings page (the rails and the rhythm between sections) | `page` inside `view::scroll_body` |
| Titled card; a card whose label line carries an accessory | `section`, `section_with`; a card alone: `card` |
| A line of copy under a card | `caption` |
| Settings row (title, optional description, control) | `setting_row`, `toggle_row`; with a leading glyph: `icon_row`; anything else: `row_frame` |
| A row the pointer is aimed at from a deep link | `highlight` |
| A few exclusive choices | `rune_kit::SegmentedControl` |
| Main and other actions | `button(…, ButtonKind::{Primary, Secondary}, …)`; one with nothing to do yet: `button_enabled` |
| An action that opens more, beside a control | `quiet_button` ("Customize…") |
| Modal shell, scrim and transition | `modal_panel` inside `modal_sheet` |
| Shortcut keys | `keycap`, `tray`, `chords`; the recorder is `panes::shortcuts::recorder` |
| Status (never clickable) | `status_dot`, `pulsing_dot` |
| A permission's Allow / Allowed control | `permission_control` |
| An app's icon | `icon_or_initial` |

## The Customize pattern

A setting with one choice most people make and a few that some people tune keeps a single row:
the choice as a segmented control, and a quiet `Customize…` button beside it that exists only while
there is something to customize. The button opens a `ModalWidth::Medium` sheet:

1. a live miniature of the thing being tuned, built from the real view (the Workspace HUD's sheet
   draws `hud::stage::miniature` with the HUD's own capsule);
2. one card of rows, on the page's rails;
3. `Done` in the footer as the default button. Escape and a press outside close it too.

Every change applies as it is made; the sheet has no Cancel. The sheet closes on its own when the
pane changes or its setting is turned off. State lives in `Ui.sheet` (`Sheet` in `main.rs`);
`panes::sheet` mounts it.

## Keycap grammar

| Part | Rule |
|---|---|
| Key | `KEYS.key_h` (22) tall with a 1.5 skirt and a 0.5 rim; legend 11/400; sinks by the skirt when pressed |
| Chord | Keys `KEYS.gap` (3) apart, modifiers in macOS order (⌃ ⌥ ⇧ ⌘), then the key |
| Tray | Recessed, `KEYS.tray_pad` (3) round the keys, so a tray is `row_h_compact` (28) tall |
| Several chords | Trays joined by "or"; none: "Not set" |
| Range | One key with the range as its legend ("1…9, 0", "A…Z"), never ten rows |
| Recorder | The tray is the button; listening shows a red dot, the modifiers held so far, then ⌫ (clear) and esc (cancel) |
| Result | A check after a saved chord; a refusal names the shortcut already using it |

Shortcut rows show the chord the base modifier produces, never a hard-coded ⌥.

## Copy

- Sentence case for every label, button, title and section label: "Reset to defaults", "Launch at
  login". Proper nouns keep their capitals: Fractile, Finder, System
  Settings, Workspace HUD, Dwindle.
- Use "…" (one character) for progress ("Restoring windows…") and for a button that opens a
  further choice ("Customize…").
- A row's description is one phrase with no closing period; it says what the current choice does
  ("Each workspace's apps"), not what the control is. Two sentences keep their periods.
- One line per control. No intro paragraphs above cards, no state words beside switches, no hint
  that repeats a label.
- Say a thing once: the version lives in the sidebar foot.
- Exception: the native menu bar menu (`src/status_item.rs`) follows the macOS title-case
  convention ("Launch at Login", "Quit Fractile").

## Enforcement

- [`controls/tokens.rs`](../agent/src/controls/tokens.rs) asserts the rails, the card widths and
  the concentric radii at compile time.
- The `design_tokens` test ([`agent/src/design_tokens.rs`](../agent/src/design_tokens.rs), run by
  `scripts/check` with the other tests) counts, per file under `agent/src`: spacing literals off
  the scale, `.radius(<literal >= 3>)`, `TextStyle::new(` and `.k.ty.` outside `controls`,
  `.min_h(<literal>)` outside `controls`, `.py(<literal>)` in a file of rows, and the kit's
  `Dialog`, `Keybind` and `ShortcutRecorder`. It skips tests and the three drawn surfaces above
  (and `playground.rs`).
- A file may exceed zero only up to its ceiling in
  [`scripts/design-token-exceptions.txt`](../scripts/design-token-exceptions.txt), which is empty
  today. Ceilings only go down: `FRACTILE_UPDATE_TOKEN_CEILINGS=1 cargo test -p fractile-agent
  design_tokens` rewrites the file and refuses to raise a number.

## Verification

```sh
# The named fixture scenes (`SCENES` in agent/src/panes.rs):
cargo run --locked -p fractile-agent -- --list-scenes
# One scene, headless:
cargo run --locked -p fractile-agent -- --size 920x660 --scene hud-customize --snap /tmp/hud.png
cargo run --locked -p fractile-agent -- --scene general --theme light --snap /tmp/general-light.png
# Several scenes, sizes and themes into one folder (<scene>-<width>[-light].png):
OUT=captures/after SIZES="860x580 920x660 1200x840" THEMES="dark light" \
  scripts/snap-set.sh general hud-customize layout rules workspaces shortcuts activity
```

Render the same scenes on `dev` first for the "before". A snap draws one frame of a fixture: it
proves layout, not the real window, input or motion, which still need a person.
