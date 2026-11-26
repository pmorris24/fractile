//! Apps by bundle ID: their names and Finder icons, and the apps running now.

#![allow(unsafe_code)]

use std::collections::HashMap;

use rune::Pixels;

/// Icons draw at this many pixels square, enough for the largest at 2x.
const ICON_PX: u32 = 64;

#[derive(Clone)]
pub struct App {
    pub name: String,
    /// `None` when the app isn't installed here; the view draws its initial.
    pub icon: Option<Pixels>,
}

#[derive(Default)]
pub struct Catalog {
    apps: HashMap<String, App>,
}

impl Catalog {
    /// The app with `bundle_id`, looked up once.
    pub fn get(&mut self, bundle_id: &str) -> App {
        self.apps
            .entry(bundle_id.to_owned())
            .or_insert_with(|| lookup(bundle_id))
            .clone()
    }

    /// Already looked up, for the view (which can't look anything up).
    pub fn cached(&self, bundle_id: &str) -> Option<&App> {
        self.apps.get(bundle_id)
    }

    pub fn insert(&mut self, bundle_id: &str, app: App) {
        self.apps.insert(bundle_id.to_owned(), app);
    }
}

/// A running app with a Dock icon.
#[derive(Clone)]
pub struct Running {
    pub bundle_id: String,
    pub name: String,
}

#[cfg(target_os = "macos")]
pub fn running() -> Vec<Running> {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
    objc2::rc::autoreleasepool(|_| {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        let mut running: Vec<Running> = apps
            .iter()
            .filter(|app| app.activationPolicy() == NSApplicationActivationPolicy::Regular)
            .filter_map(|app| {
                Some(Running {
                    bundle_id: app.bundleIdentifier()?.to_string(),
                    name: app.localizedName()?.to_string(),
                })
            })
            .filter(|app| app.bundle_id != "app.fractile")
            .collect();
        running.sort_by_key(|app| app.name.to_lowercase());
        running.dedup_by(|a, b| a.bundle_id == b.bundle_id);
        running
    })
}

#[cfg(not(target_os = "macos"))]
pub fn running() -> Vec<Running> {
    Vec::new()
}

/// The apps a fixture has running, so a still's Add menu doesn't list this Mac's.
pub fn fixture_running() -> Vec<Running> {
    [
        ("com.anthropic.claudefordesktop", "Claude"),
        ("com.apple.Notes", "Notes"),
        ("com.apple.Safari", "Safari"),
        ("com.tinyspeck.slackmacgap", "Slack"),
        ("com.apple.Stickies", "Stickies"),
    ]
    .map(|(bundle_id, name)| Running {
        bundle_id: bundle_id.to_owned(),
        name: name.to_owned(),
    })
    .into()
}

const KNOWN_WINDOW_MANAGERS: &[(&str, &str)] = &[
    ("bobko.aerospace", "AeroSpace"),
    ("com.amethyst.Amethyst", "Amethyst"),
    ("com.knollsoft.Rectangle", "Rectangle"),
    ("com.knollsoft.Hookshot", "Rectangle Pro"),
    ("com.crowdcafe.windowmagnet", "Magnet"),
    ("com.hegenberg.BetterSnapTool", "BetterSnapTool"),
    ("com.KaiMrgn.Loop", "Loop"),
    ("co.soffes.Swish", "Swish"),
    ("com.highlyopinionated.swish", "Swish"),
];

pub fn window_manager_name(
    bundle_id: Option<&str>,
    executable: Option<&str>,
) -> Option<&'static str> {
    if let Some(bundle_id) = bundle_id {
        for &(known_id, name) in KNOWN_WINDOW_MANAGERS {
            if bundle_id.eq_ignore_ascii_case(known_id) {
                return Some(name);
            }
        }
    }
    if executable.is_some_and(|name| name.eq_ignore_ascii_case("yabai")) {
        return Some("yabai");
    }
    None
}

/// Other macOS window managers currently running alongside Fractile.
#[cfg(target_os = "macos")]
pub fn conflicting_window_managers() -> Vec<String> {
    use objc2_app_kit::NSWorkspace;
    objc2::rc::autoreleasepool(|_| {
        let apps = NSWorkspace::sharedWorkspace().runningApplications();
        let mut names: Vec<String> = apps
            .iter()
            .filter_map(|app| {
                let bundle_id = app.bundleIdentifier().map(|id| id.to_string());
                let executable = app
                    .executableURL()
                    .and_then(|url| url.lastPathComponent())
                    .map(|name| name.to_string());
                window_manager_name(bundle_id.as_deref(), executable.as_deref()).map(str::to_owned)
            })
            .collect();
        names.sort();
        names.dedup();
        names
    })
}

#[cfg(not(target_os = "macos"))]
pub fn conflicting_window_managers() -> Vec<String> {
    Vec::new()
}

/// Asks the running window manager called `name` to quit. Returns whether one was asked.
#[cfg(target_os = "macos")]
pub fn quit_window_manager(name: &str) -> bool {
    use objc2_app_kit::NSWorkspace;
    objc2::rc::autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .filter(|app| {
                let bundle_id = app.bundleIdentifier().map(|id| id.to_string());
                let executable = app
                    .executableURL()
                    .and_then(|url| url.lastPathComponent())
                    .map(|name| name.to_string());
                window_manager_name(bundle_id.as_deref(), executable.as_deref()) == Some(name)
            })
            // Every match is asked, so no short-circuit.
            .filter(|app| app.terminate())
            .count()
            > 0
    })
}

/// The app's `LSApplicationCategoryType` from its `Info.plist`, such as
/// `public.app-category.developer-tools`. `None` when it isn't installed or declares none.
#[cfg(target_os = "macos")]
pub fn category(bundle_id: &str) -> Option<String> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSString};
    objc2::rc::autoreleasepool(|_| {
        let url = NSWorkspace::sharedWorkspace()
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
        let value = NSBundle::bundleWithURL(&url)?
            .objectForInfoDictionaryKey(&NSString::from_str("LSApplicationCategoryType"))?;
        let category = value.downcast::<NSString>().ok()?.to_string();
        (!category.is_empty()).then_some(category)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn category(_: &str) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
fn lookup(bundle_id: &str) -> App {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSFileManager, NSString};
    objc2::rc::autoreleasepool(|_| {
        let workspace = NSWorkspace::sharedWorkspace();
        let Some(path) = workspace
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
            .and_then(|url| url.path())
        else {
            return App {
                name: fallback_name(bundle_id),
                icon: None,
            };
        };
        let name = NSFileManager::defaultManager()
            .displayNameAtPath(&path)
            .to_string();
        let name = name.strip_suffix(".app").unwrap_or(&name).to_owned();
        let icon = raster::fitted(&workspace.iconForFile(&path), ICON_PX).map(|(w, h, rgba)| {
            let pixels = Pixels::new();
            pixels.set(w, h, rgba);
            pixels
        });
        App { name, icon }
    })
}

#[cfg(not(target_os = "macos"))]
fn lookup(bundle_id: &str) -> App {
    App {
        name: fallback_name(bundle_id),
        icon: None,
    }
}

/// `com.tinyspeck.slackmacgap` → `Slackmacgap`: the last part of the ID, for apps not installed.
pub fn fallback_name(bundle_id: &str) -> String {
    let last = bundle_id.rsplit('.').next().unwrap_or(bundle_id);
    let mut chars = last.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod raster {
    use objc2::{AllocAnyThread, rc::Retained};
    use objc2_app_kit::{
        NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext, NSImage,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// `image` drawn into a `px` square as straight-alpha RGBA.
    pub fn fitted(image: &NSImage, px: u32) -> Option<(u32, u32, Vec<u8>)> {
        let edge = f64::from(px);
        let bitmap = draw(px as isize, || {
            image.drawInRect_fromRect_operation_fraction(
                NSRect::new(NSPoint::new(0., 0.), NSSize::new(edge, edge)),
                NSRect::ZERO,
                NSCompositingOperation::SourceOver,
                1.0,
            );
        })?;
        let stride = usize::try_from(bitmap.bytesPerRow()).ok()?;
        let row = px as usize * 4;
        let data = bitmap.bitmapData();
        if data.is_null() || stride < row {
            return None;
        }
        // SAFETY: AppKit owns `px` rows of `stride` bytes while `bitmap` lives.
        let bytes = unsafe { std::slice::from_raw_parts(data, stride * px as usize) };
        let mut out = Vec::with_capacity(row * px as usize);
        for line in bytes.chunks_exact(stride) {
            out.extend_from_slice(&line[..row]);
        }
        // The bitmap is premultiplied.
        for pixel in out.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            if alpha > 0 && alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        Some((px, px, out))
    }

    fn draw(edge: isize, paint: impl FnOnce()) -> Option<Retained<NSBitmapImageRep>> {
        struct Restore;
        impl Drop for Restore {
            fn drop(&mut self) {
                NSGraphicsContext::restoreGraphicsState_class();
            }
        }
        // SAFETY: Null planes ask AppKit to allocate a packed RGBA buffer it owns.
        let bitmap = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                edge,
                edge,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
        NSGraphicsContext::saveGraphicsState_class();
        let restore = Restore;
        NSGraphicsContext::setCurrentContext(Some(&context));
        paint();
        context.flushGraphics();
        drop(restore);
        Some(bitmap)
    }
}
