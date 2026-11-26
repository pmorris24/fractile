use std::{cell::RefCell, sync::mpsc::Sender};

use block2::RcBlock;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, msg_send,
    rc::Retained,
    runtime::{Bool, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu,
    NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusBarButton, NSStatusItem, NSStringDrawing,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};

use crate::{
    WorkspaceId,
    app::{Action, UiHudLayout, WorkerAction, hud_preview_rect},
    login_item, macos,
};

mod workspaces;

pub(crate) use workspaces::{DisplayWorkspaces, MenuWorkspace};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceLayout {
    pub(crate) workspace: String,
    pub(crate) mode: UiHudLayout,
    /// On a portrait display (`[labs] physical_displays`): the glyph is tall and `mode`'s tiles
    /// are in [`crate::app::HUD_PORTRAIT_PREVIEW_RECT`] space.
    pub(crate) portrait: bool,
    /// The display's label for VoiceOver, with `[labs] physical_displays` on.
    pub(crate) display: Option<String>,
}

struct TargetIvars {
    actions: Sender<Action>,
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSMenuDelegate for Target {
        /// Shows only the workspaces of the display whose menu bar the menu opens from.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, _menu: &NSMenu) {
            show_pointer_display_workspaces();
        }
    }

    impl Target {
        #[unsafe(method(openSetup:))]
        fn open_setup(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::OpenSetup));
        }

        #[unsafe(method(toggleAutomatic:))]
        fn toggle_automatic(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleAutomatic));
        }

        #[unsafe(method(toggleHud:))]
        fn toggle_hud(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleHud));
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::OpenSettings));
        }

        #[unsafe(method(toggleLaunchAtLogin:))]
        fn toggle_launch_at_login(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::ToggleLaunchAtLogin));
        }

        #[unsafe(method(switchWorkspace:))]
        fn switch_workspace(&self, sender: &NSMenuItem) {
            if let Ok(index) = usize::try_from(sender.tag()) {
                drop(self.ivars().actions.send(Action::Worker(
                    WorkerAction::SelectWorkspace(WorkspaceId::from_index(index)),
                )));
            }
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &NSObject) {
            drop(self.ivars().actions.send(Action::Quit));
        }
    }
);

impl Target {
    fn new(actions: Sender<Action>, marker: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(marker).set_ivars(TargetIvars { actions });
        // SAFETY: `NSObject`'s `-init` takes no arguments and returns an initialized `Self`.
        unsafe { msg_send![super(this), init] }
    }
}

struct Controller {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<Target>,
    button: Retained<NSStatusBarButton>,
    status: Retained<NSMenuItem>,
    setup: Retained<NSMenuItem>,
    setup_separator: Retained<NSMenuItem>,
    automatic: Retained<NSMenuItem>,
    hud: Retained<NSMenuItem>,
    launch_at_login: Retained<NSMenuItem>,
    status_title: String,
    button_state: ButtonState,
    rendered_visual: Option<ButtonVisual>,
    workspace_menu: Vec<DisplayWorkspaces>,
    workspace_items: Vec<workspaces::DisplayItem>,
}

thread_local! {
    static CONTROLLER: RefCell<Option<Controller>> = const { RefCell::new(None) };
}

pub fn install(sender: Sender<Action>) -> Result<(), String> {
    let marker = MainThreadMarker::new().ok_or("status item requires the main thread")?;
    let target = Target::new(sender, marker);
    let menu = NSMenu::initWithTitle(NSMenu::alloc(marker), &NSString::from_str("Spaceman"));
    let status = plain_item("Starting...", marker);
    status.setEnabled(false);
    status.setHidden(true);
    menu.addItem(&status);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    let setup = action_item("Setup Tour…", &target, sel!(openSetup:), "", marker);
    setup.setHidden(true);
    menu.addItem(&setup);
    let setup_separator = NSMenuItem::separatorItem(marker);
    setup_separator.setHidden(true);
    menu.addItem(&setup_separator);
    let automatic = action_item("Pause Tiling", &target, sel!(toggleAutomatic:), "", marker);
    menu.addItem(&automatic);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    let hud = action_item("Workspace HUD", &target, sel!(toggleHud:), "", marker);
    hud.setState(NSControlStateValueOn);
    menu.addItem(&hud);
    let launch_at_login = action_item(
        "Launch at Login",
        &target,
        sel!(toggleLaunchAtLogin:),
        "",
        marker,
    );
    menu.addItem(&launch_at_login);
    menu.addItem(&NSMenuItem::separatorItem(marker));
    menu.addItem(&action_item(
        "Settings...",
        &target,
        sel!(openSettings:),
        ",",
        marker,
    ));
    menu.addItem(&NSMenuItem::separatorItem(marker));
    menu.addItem(&action_item(
        "Quit Spaceman",
        &target,
        sel!(quit:),
        "q",
        marker,
    ));

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let button = item
        .button(marker)
        .ok_or("status item button is unavailable")?;
    menu.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    item.setMenu(Some(&menu));

    CONTROLLER.with(|controller| {
        *controller.borrow_mut() = Some(Controller {
            item,
            menu,
            target,
            button,
            status,
            setup,
            setup_separator,
            automatic,
            hud,
            launch_at_login,
            status_title: String::new(),
            button_state: ButtonState::new(),
            rendered_visual: None,
            workspace_menu: Vec::new(),
            workspace_items: Vec::new(),
        });
    });
    Ok(())
}

/// Shows or hides the whole menu bar icon: hidden for `settings.menu_bar_icon = "off"`
/// (with `[labs] menu_bar_icon_style`). Settings stays reachable by opening Spaceman again.
pub fn set_visible(visible: bool) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref()
            && controller.item.isVisible() != visible
        {
            controller.item.setVisible(visible);
        }
    });
}

pub fn set_status(status: &str) {
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        if let Some(controller) = controller.as_mut()
            && controller.status_title != status
        {
            controller.status.setHidden(false);
            controller.status.setTitle(&NSString::from_str(status));
            status.clone_into(&mut controller.status_title);
        }
    });
}

pub fn clear_status() {
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        if let Some(controller) = controller.as_mut() {
            controller.status.setHidden(true);
            controller.status_title.clear();
        }
    });
}

pub fn set_accessibility(trusted: bool) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller.setup.setHidden(trusted);
            controller.setup_separator.setHidden(trusted);
        }
    });
}

pub fn set_hud(enabled: bool) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            controller.hud.setState(if enabled {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    });
}

pub fn set_automatic(enabled: bool) {
    if !enabled {
        // Switching is ignored while paused, and the list goes stale.
        set_workspace_menu(&[]);
    }
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow_mut().as_mut() {
            controller
                .automatic
                .setTitle(&NSString::from_str(if enabled {
                    "Pause Tiling"
                } else {
                    "Resume Tiling"
                }));
            controller.button_state.set_automatic(enabled);
            apply_button_visual(controller);
        }
    });
}

pub fn set_launch_at_login(status: login_item::Status) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow().as_ref() {
            let (title, state) = match status {
                login_item::Status::Disabled => ("Launch at Login", NSControlStateValueOff),
                login_item::Status::Enabled => ("Launch at Login", NSControlStateValueOn),
                login_item::Status::RequiresApproval => (
                    "Launch at Login (Approval Required...)",
                    NSControlStateValueOff,
                ),
            };
            controller
                .launch_at_login
                .setTitle(&NSString::from_str(title));
            controller.launch_at_login.setState(state);
        }
    });
}

pub fn set_workspace_numbers(workspaces: &[u8]) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow_mut().as_mut() {
            controller.button_state.set_numbers(workspaces);
            apply_button_visual(controller);
        }
    });
}

pub fn set_workspace_layouts(layouts: &[WorkspaceLayout]) {
    CONTROLLER.with(|controller| {
        if let Some(controller) = controller.borrow_mut().as_mut() {
            controller.button_state.set_layouts(layouts);
            apply_button_visual(controller);
        }
    });
}

fn apply_button_visual(controller: &mut Controller) {
    let visual = controller.button_state.visual();
    if controller.rendered_visual.as_ref() == Some(&visual) {
        return;
    }
    controller.button.setAlphaValue(visual.alpha);
    match &visual.content {
        ButtonContent::Paused => {
            let image = paused_glyph_image();
            controller.button.setTitle(&NSString::new());
            controller.button.setImage(Some(&image));
        }
        ButtonContent::Layouts(layouts) => {
            let image = workspace_layout_image(layouts);
            controller.button.setTitle(&NSString::new());
            controller.button.setImage(Some(&image));
        }
        ButtonContent::Numbers(workspaces) => {
            let title = format_workspace_numbers(workspaces);
            controller.button.setImage(None);
            controller.button.setTitle(&NSString::from_str(&title));
        }
    }
    controller.rendered_visual = Some(visual);
}

fn format_workspace_numbers(workspaces: &[u8]) -> String {
    if workspaces.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            workspaces
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(" · ")
        )
    }
}

/// Lists `displays`' workspaces above the tiling toggle.
pub fn set_workspace_menu(displays: &[DisplayWorkspaces]) {
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    CONTROLLER.with(|controller| {
        let mut controller = controller.borrow_mut();
        let Some(controller) = controller.as_mut() else {
            return;
        };
        if controller.workspace_menu == displays {
            return;
        }
        for (item, _) in controller.workspace_items.drain(..) {
            controller.menu.removeItem(&item);
        }
        let items = workspaces::items(displays, &controller.target, marker);
        let start = controller.menu.indexOfItem(&controller.automatic);
        for (offset, (item, _)) in (start..).zip(&items) {
            controller.menu.insertItem_atIndex(item, offset);
        }
        controller.workspace_items = items;
        controller.workspace_menu = displays.to_vec();
    });
}

/// Hides every workspace section except the one for the display under the pointer, or shows them
/// all when that display is unknown or has no section.
fn show_pointer_display_workspaces() {
    let pointer = macos::display_under_pointer();
    CONTROLLER.with(|controller| {
        let Ok(controller) = controller.try_borrow() else {
            return;
        };
        let Some(controller) = controller.as_ref() else {
            return;
        };
        let shown = workspaces::shown_display(
            controller
                .workspace_menu
                .iter()
                .map(|display| display.display_id.as_str()),
            pointer.as_deref(),
        );
        for (item, display) in &controller.workspace_items {
            let hidden = match (display, shown) {
                (Some(display), Some(shown)) => display != shown,
                _ => false,
            };
            item.setHidden(hidden);
        }
    });
}

fn workspace_layout_image(layouts: &[WorkspaceLayout]) -> Retained<NSImage> {
    let labels = layouts
        .iter()
        .map(|layout| NSString::from_str(&layout.workspace))
        .collect::<Vec<_>>();
    let label_widths = labels
        .iter()
        .map(|label| {
            // SAFETY: `sizeWithAttributes(None)` measures `label` using AppKit's default font.
            unsafe { label.sizeWithAttributes(None) }.width.ceil()
        })
        .collect::<Vec<_>>();
    let cells = layouts
        .iter()
        .zip(&label_widths)
        .map(|(layout, label_width)| (glyph_size(layout.portrait).width, *label_width))
        .collect::<Vec<_>>();
    let width = workspace_layout_image_width(&cells);
    let description = accessibility_description(layouts);
    let layouts = layouts.to_vec();
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(width, 16.0),
        false,
        &RcBlock::new(move |_: NSRect| {
            draw_workspace_layouts(&layouts, &labels, &label_widths);
            Bool::YES
        }),
    );
    image.setTemplate(true);
    if let Some(description) = description {
        image.setAccessibilityDescription(Some(&NSString::from_str(&description)));
    }
    image
}

/// What VoiceOver reads for the glyphs when they name their displays, such as
/// "Workspace 6 on DELL U2720Q (left), workspace 1 on Studio Display".
fn accessibility_description(layouts: &[WorkspaceLayout]) -> Option<String> {
    if layouts.iter().all(|layout| layout.display.is_none()) {
        return None;
    }
    let spoken = layouts
        .iter()
        .map(|layout| match &layout.display {
            Some(display) => format!("workspace {} on {display}", layout.workspace),
            None => format!("workspace {}", layout.workspace),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut chars = spoken.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
}

const ACTIVE_ALPHA: f64 = 1.0;
const PAUSED_ALPHA: f64 = 0.45;
const PAUSED_ACCESSIBILITY_DESCRIPTION: &str = "Spaceman paused";
const PAUSE_BAR_WIDTH: f64 = 2.0;
const PAUSE_BAR_HEIGHT: f64 = 5.5;
const PAUSE_BAR_GAP: f64 = 2.0;
const PAUSE_BAR_RADIUS: f64 = 0.75;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ButtonContent {
    Paused,
    Layouts(Vec<WorkspaceLayout>),
    Numbers(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq)]
struct ButtonVisual {
    alpha: f64,
    content: ButtonContent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ButtonState {
    automatic: bool,
    has_layouts: bool,
    layouts: Vec<WorkspaceLayout>,
    numbers: Vec<u8>,
}

impl ButtonState {
    const fn new() -> Self {
        Self {
            automatic: true,
            has_layouts: false,
            layouts: Vec::new(),
            numbers: Vec::new(),
        }
    }

    const fn set_automatic(&mut self, enabled: bool) {
        self.automatic = enabled;
    }

    fn set_numbers(&mut self, workspaces: &[u8]) {
        self.has_layouts = false;
        self.layouts.clear();
        self.numbers = workspaces.to_vec();
    }

    fn set_layouts(&mut self, layouts: &[WorkspaceLayout]) {
        self.has_layouts = true;
        self.layouts = layouts.to_vec();
    }

    fn visual(&self) -> ButtonVisual {
        if !self.automatic {
            return ButtonVisual {
                alpha: PAUSED_ALPHA,
                content: ButtonContent::Paused,
            };
        }
        let content = if self.has_layouts {
            ButtonContent::Layouts(self.layouts.clone())
        } else {
            ButtonContent::Numbers(self.numbers.clone())
        };
        ButtonVisual {
            alpha: ACTIVE_ALPHA,
            content,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PausedGlyphGeometry {
    image_width: f64,
    frame: NSRect,
    left_bar: NSRect,
    right_bar: NSRect,
}

fn paused_glyph_geometry() -> PausedGlyphGeometry {
    let size = glyph_size(false);
    let frame = NSRect::new(
        NSPoint::new(LAYOUT_START_X, (16.0 - size.height) / 2.0),
        size,
    );
    let bars_total_w = PAUSE_BAR_WIDTH * 2.0 + PAUSE_BAR_GAP;
    let bars_x = frame.origin.x + (size.width - bars_total_w) / 2.0;
    let bars_y = frame.origin.y + (size.height - PAUSE_BAR_HEIGHT) / 2.0;
    PausedGlyphGeometry {
        image_width: LAYOUT_START_X + size.width + TRAILING_PADDING,
        frame,
        left_bar: NSRect::new(
            NSPoint::new(bars_x, bars_y),
            NSSize::new(PAUSE_BAR_WIDTH, PAUSE_BAR_HEIGHT),
        ),
        right_bar: NSRect::new(
            NSPoint::new(bars_x + PAUSE_BAR_WIDTH + PAUSE_BAR_GAP, bars_y),
            NSSize::new(PAUSE_BAR_WIDTH, PAUSE_BAR_HEIGHT),
        ),
    }
}

fn paused_glyph_image() -> Retained<NSImage> {
    let geometry = paused_glyph_geometry();
    let image = NSImage::imageWithSize_flipped_drawingHandler(
        NSSize::new(geometry.image_width, 16.0),
        false,
        &RcBlock::new(move |_: NSRect| {
            draw_paused_glyph(&geometry);
            Bool::YES
        }),
    );
    image.setTemplate(true);
    image.setAccessibilityDescription(Some(&NSString::from_str(PAUSED_ACCESSIBILITY_DESCRIPTION)));
    image
}

fn draw_paused_glyph(geometry: &PausedGlyphGeometry) {
    NSColor::blackColor().setStroke();
    NSColor::blackColor().setFill();
    let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(geometry.frame, 2.5, 2.5);
    outline.setLineWidth(1.25);
    outline.stroke();
    for bar in [geometry.left_bar, geometry.right_bar] {
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
            bar,
            PAUSE_BAR_RADIUS,
            PAUSE_BAR_RADIUS,
        )
        .fill();
    }
}

// Drawing offsets shared by the image size and `draw_workspace_layouts`.
const LAYOUT_START_X: f64 = 0.5;
/// From a glyph's right edge to its label.
const LABEL_GAP: f64 = 4.0;
const LAYOUT_GAP: f64 = 5.0;
const TRAILING_PADDING: f64 = 1.0;
/// A landscape glyph; a portrait one is the same box on its side.
const GLYPH_LONG: f64 = 15.0;
const GLYPH_SHORT: f64 = 11.0;

/// A layout glyph's size: tall on a portrait display.
const fn glyph_size(portrait: bool) -> NSSize {
    if portrait {
        NSSize::new(GLYPH_SHORT, GLYPH_LONG)
    } else {
        NSSize::new(GLYPH_LONG, GLYPH_SHORT)
    }
}

/// Width that holds every `(glyph width, label width)` cell, ending one point past the last
/// label.
fn workspace_layout_image_width(cells: &[(f64, f64)]) -> f64 {
    if cells.is_empty() {
        return 0.0;
    }
    let steps = cells.len() - 1;
    LAYOUT_START_X
        + cells
            .iter()
            .map(|(glyph, label)| glyph + LABEL_GAP + label)
            .sum::<f64>()
        + steps as f64 * LAYOUT_GAP
        + TRAILING_PADDING
}

fn draw_workspace_layouts(
    layouts: &[WorkspaceLayout],
    labels: &[Retained<NSString>],
    label_widths: &[f64],
) {
    NSColor::blackColor().setStroke();
    NSColor::blackColor().setFill();
    let mut x = LAYOUT_START_X;
    for ((layout, label), label_width) in
        layouts.iter().zip(labels).zip(label_widths.iter().copied())
    {
        let size = glyph_size(layout.portrait);
        // Centred on the 16-point image: y 2.5 for a landscape glyph, 0.5 for a portrait one.
        let frame = NSRect::new(NSPoint::new(x, (16.0 - size.height) / 2.0), size);
        let preview = hud_preview_rect(layout.portrait);
        let outline = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(frame, 2.5, 2.5);
        match &layout.mode {
            UiHudLayout::Fullscreen => outline.fill(),
            UiHudLayout::Split(tiles) => {
                outline.setLineWidth(1.25);
                outline.stroke();
                for tile in tiles {
                    let (preview_w, preview_h) =
                        (f64::from(preview.width), f64::from(preview.height));
                    let tile = NSRect::new(
                        NSPoint::new(
                            x + f64::from(tile.x) / preview_w * size.width,
                            // Preview frames are top-down; the image is unflipped.
                            frame.origin.y
                                + (preview_h - f64::from(tile.y) - f64::from(tile.height))
                                    / preview_h
                                    * size.height,
                        ),
                        NSSize::new(
                            f64::from(tile.width) / preview_w * size.width,
                            f64::from(tile.height) / preview_h * size.height,
                        ),
                    );
                    let tile = NSBezierPath::bezierPathWithRect(tile);
                    tile.setLineWidth(0.75);
                    tile.stroke();
                }
            }
        }
        // SAFETY: Called inside `NSImage` drawing handler with a active graphics context.
        unsafe {
            label.drawAtPoint_withAttributes(NSPoint::new(x + size.width + LABEL_GAP, 0.5), None);
        };
        x += size.width + LABEL_GAP + LAYOUT_GAP + label_width;
    }
}

fn plain_item(title: &str, marker: MainThreadMarker) -> Retained<NSMenuItem> {
    // SAFETY: `NSMenuItem` is allocated on the main thread with no target/action selector.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(marker),
            &NSString::from_str(title),
            None,
            &NSString::new(),
        )
    }
}

fn action_item(
    title: &str,
    target: &Target,
    action: objc2::runtime::Sel,
    key: &str,
    marker: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: `action` matches a method declared on `Target` taking a single sender object.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(marker),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    // SAFETY: `target` is retained inside `Controller` for the lifetime of the menu.
    unsafe { item.setTarget(Some(target)) };
    item
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_width_covers_the_last_label() {
        let (long, short) = (GLYPH_LONG, GLYPH_SHORT);
        for cells in [
            vec![(long, 7.0)],
            vec![(long, 7.0), (short, 8.0)],
            vec![(short, 7.0), (long, 14.0), (long, 8.0), (short, 7.0)],
        ] {
            let mut x = LAYOUT_START_X;
            let mut label_end = 0.0;
            for (glyph, width) in &cells {
                label_end = x + glyph + LABEL_GAP + width;
                x += glyph + LABEL_GAP + LAYOUT_GAP + width;
            }
            assert!(workspace_layout_image_width(&cells) >= label_end);
        }
        assert_eq!(workspace_layout_image_width(&[(long, 7.0)]), 27.5);
        assert_eq!(workspace_layout_image_width(&[]), 0.0);
    }

    #[test]
    fn portrait_glyphs_are_tall_and_fit_the_menu_bar_image() {
        let tall = glyph_size(true);
        assert!(tall.height > tall.width && tall.height <= 16.0);
        let wide = glyph_size(false);
        assert_eq!((wide.width, wide.height), (15.0, 11.0));
    }

    #[test]
    fn voiceover_names_each_workspace_display_only_when_known() {
        let layout = |workspace: &str, display: Option<&str>| WorkspaceLayout {
            workspace: workspace.into(),
            mode: UiHudLayout::Fullscreen,
            portrait: false,
            display: display.map(str::to_owned),
        };
        assert_eq!(accessibility_description(&[layout("1", None)]), None);
        assert_eq!(
            accessibility_description(&[
                layout("6", Some("DELL U2720Q (left)")),
                layout("1", Some("Studio Display")),
            ])
            .as_deref(),
            Some("Workspace 6 on DELL U2720Q (left), workspace 1 on Studio Display")
        );
    }

    #[test]
    fn paused_state_dims_icon_with_pause_glyph_and_restores_full_brightness_on_resume() {
        let layout = WorkspaceLayout {
            workspace: "1".into(),
            mode: UiHudLayout::Fullscreen,
            portrait: false,
            display: None,
        };
        let mut state = ButtonState::new();
        state.set_numbers(&[1, 6]);
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: ACTIVE_ALPHA,
                content: ButtonContent::Numbers(vec![1, 6]),
            }
        );

        // Pausing before layouts arrive switches to the dimmed paused icon.
        state.set_automatic(false);
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: PAUSED_ALPHA,
                content: ButtonContent::Paused,
            }
        );

        // Resuming immediately restores full brightness and the prior workspace numbers.
        state.set_automatic(true);
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: ACTIVE_ALPHA,
                content: ButtonContent::Numbers(vec![1, 6]),
            }
        );

        // Active layouts render at full brightness; pausing switches to the paused glyph.
        state.set_layouts(std::slice::from_ref(&layout));
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: ACTIVE_ALPHA,
                content: ButtonContent::Layouts(vec![layout.clone()]),
            }
        );
        state.set_automatic(false);
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: PAUSED_ALPHA,
                content: ButtonContent::Paused,
            }
        );

        // Layout updates received while paused are remembered without overriding the paused icon.
        let compact = WorkspaceLayout {
            workspace: "3".into(),
            mode: UiHudLayout::Fullscreen,
            portrait: false,
            display: None,
        };
        state.set_layouts(std::slice::from_ref(&compact));
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: PAUSED_ALPHA,
                content: ButtonContent::Paused,
            }
        );

        // Resuming restores full brightness and the latest layouts immediately.
        state.set_automatic(true);
        assert_eq!(
            state.visual(),
            ButtonVisual {
                alpha: ACTIVE_ALPHA,
                content: ButtonContent::Layouts(vec![compact]),
            }
        );
    }

    #[test]
    fn paused_glyph_bars_sit_centered_inside_the_rounded_frame() {
        let g = paused_glyph_geometry();
        assert_eq!(
            g.image_width,
            LAYOUT_START_X + GLYPH_LONG + TRAILING_PADDING
        );
        assert!(g.frame.origin.y >= 0.0 && g.frame.origin.y + g.frame.size.height <= 16.0);
        assert!(g.left_bar.origin.x > g.frame.origin.x);
        assert!(g.left_bar.origin.x + g.left_bar.size.width < g.right_bar.origin.x);
        assert!(
            g.right_bar.origin.x + g.right_bar.size.width < g.frame.origin.x + g.frame.size.width
        );
        assert!(g.left_bar.origin.y > g.frame.origin.y);
        assert!(
            g.left_bar.origin.y + g.left_bar.size.height < g.frame.origin.y + g.frame.size.height
        );
        // Symmetric horizontal margins inside the display frame.
        let left_margin = g.left_bar.origin.x - g.frame.origin.x;
        let right_margin = (g.frame.origin.x + g.frame.size.width)
            - (g.right_bar.origin.x + g.right_bar.size.width);
        assert!((left_margin - right_margin).abs() < 1e-6);
    }
}
