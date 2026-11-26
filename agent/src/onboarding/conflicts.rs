//! What would fight Fractile for the same windows, read only when it is really on: Stage
//! Manager, macOS's own edge tiling, and other window managers that are running.
//!
//! The two macOS features live in the `com.apple.WindowManager` preferences. Turn off writes
//! the same key System Settings writes, so it is undone there.

use crate::apps;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Conflict {
    StageManager,
    EdgeTiling,
    /// Another window manager that is running, by name.
    WindowManager(String),
}

impl Conflict {
    pub fn label(&self) -> String {
        match self {
            Self::StageManager => "Stage Manager is on".into(),
            Self::EdgeTiling => "macOS edge tiling is on".into(),
            Self::WindowManager(name) => format!("{name} is running"),
        }
    }

    /// What its button does.
    pub fn action(&self) -> &'static str {
        match self {
            Self::StageManager | Self::EdgeTiling => "Turn off",
            Self::WindowManager(_) => "Quit",
        }
    }

    pub fn key(&self) -> String {
        match self {
            Self::StageManager => "stage-manager".into(),
            Self::EdgeTiling => "edge-tiling".into(),
            Self::WindowManager(name) => format!("wm-{}", name.to_lowercase()),
        }
    }

    #[cfg(target_os = "macos")]
    fn preference(&self) -> Option<&'static str> {
        match self {
            Self::StageManager => Some(STAGE_MANAGER),
            Self::EdgeTiling => Some(EDGE_TILING),
            Self::WindowManager(_) => None,
        }
    }
}

#[cfg(target_os = "macos")]
const DOMAIN: &str = "com.apple.WindowManager";
#[cfg(target_os = "macos")]
const STAGE_MANAGER: &str = "GloballyEnabled";
#[cfg(target_os = "macos")]
const EDGE_TILING: &str = "EnableTilingByEdgeDrag";
/// macOS 15 added edge tiling, on until someone turns it off.
#[cfg(target_os = "macos")]
const EDGE_TILING_SINCE: isize = 15;

/// Whether a `com.apple.WindowManager` switch is on: its stored value, or `unset` when it was
/// never touched.
pub fn switch_on(stored: Option<bool>, unset: bool) -> bool {
    stored.unwrap_or(unset)
}

/// Everything in the way right now. Empty on a Mac with none of it on.
#[cfg(target_os = "macos")]
pub fn current() -> Vec<Conflict> {
    use objc2_foundation::{NSProcessInfo, NSString};
    let mut found = Vec::new();
    objc2::rc::autoreleasepool(|_| {
        let Some(defaults) = window_manager_defaults() else {
            return;
        };
        let stored = |key: &str| {
            let key = NSString::from_str(key);
            defaults
                .objectForKey(&key)
                .map(|_| defaults.boolForKey(&key))
        };
        if switch_on(stored(STAGE_MANAGER), false) {
            found.push(Conflict::StageManager);
        }
        let has_edge_tiling = NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            >= EDGE_TILING_SINCE;
        if has_edge_tiling && switch_on(stored(EDGE_TILING), true) {
            found.push(Conflict::EdgeTiling);
        }
    });
    found.extend(
        apps::conflicting_window_managers()
            .into_iter()
            .map(Conflict::WindowManager),
    );
    found
}

#[cfg(not(target_os = "macos"))]
pub fn current() -> Vec<Conflict> {
    apps::conflicting_window_managers()
        .into_iter()
        .map(Conflict::WindowManager)
        .collect()
}

#[cfg(target_os = "macos")]
fn window_manager_defaults() -> Option<objc2::rc::Retained<objc2_foundation::NSUserDefaults>> {
    use objc2::AllocAnyThread;
    use objc2_foundation::{NSString, NSUserDefaults};
    NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(DOMAIN)))
}

/// Turns a macOS switch off, or asks another window manager to quit. Returns whether it did.
#[cfg(target_os = "macos")]
pub fn resolve(conflict: &Conflict) -> bool {
    use objc2_foundation::NSString;
    objc2::rc::autoreleasepool(|_| match conflict {
        Conflict::WindowManager(name) => apps::quit_window_manager(name),
        switch => {
            let (Some(key), Some(defaults)) = (switch.preference(), window_manager_defaults())
            else {
                return false;
            };
            defaults.setBool_forKey(false, &NSString::from_str(key));
            true
        }
    })
}

#[cfg(not(target_os = "macos"))]
pub fn resolve(_: &Conflict) -> bool {
    false
}
