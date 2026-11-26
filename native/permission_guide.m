#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#import <CoreImage/CoreImage.h>
#import <QuartzCore/QuartzCore.h>
#import <stdint.h>
#import <stdlib.h>
#import <unistd.h>

typedef struct {
    double x;
    double y;
} FractileGuidePoint;

typedef struct {
    double width;
    double height;
} FractileGuideSize;

typedef struct {
    double x;
    double y;
    double width;
    double height;
} FractileGuideRect;

typedef struct {
    int32_t pid;
    int32_t layer;
    double alpha;
    FractileGuideRect frame;
    bool is_settings_owner;
    bool is_system_prompt_owner;
    bool is_ignored_chrome;
} FractileGuideWindowInfo;

typedef struct {
    bool has_settings_frame;
    FractileGuideRect settings_frame;
    bool has_blocking_sheet_or_prompt;
} FractileGuideWindowAnalysis;

@class FractileGuideDelegate;

static NSPanel *guidePanel;
static FractileGuideDelegate *guideDelegate;
static NSTimer *guideTimer;
static BOOL sawSettingsWindow;
static BOOL guideUserMoved;
static BOOL guidePlacingProgrammatically;
/// The Accessibility pane the guide points at, reopened when Settings is closed
/// under it, at most `guideReopenLimit` times before the guide gives up.
static NSURL *guideSettingsURL;
static NSUInteger guideReopens;
static const NSUInteger guideReopenLimit = 2;
static NSView *guideTile;

/// The tile flies on its own from the click in Fractile's window to its place
/// in the guide, so the eye can follow it to System Settings.
static NSPanel *flightPanel;
static CALayer *flightLayer;
static NSTimer *flightTimer;
static NSPoint flightOrigin;
static CFTimeInterval flightShownAt;
static CFTimeInterval flightStartedAt;
static const CGFloat flightPadding = 40;
static const CFTimeInterval flightLift = 0.18;
static const CFTimeInterval flightDuration = 0.62;
/// Settings that has not opened by then gets no flight.
static const CFTimeInterval flightPatience = 4;

static NSString *const settingsBundleID = @"com.apple.systempreferences";
/// Bumped by every pane request or guide close, so an older request's retries
/// stand down.
static NSUInteger settingsRequest;

@interface FractileDragView : NSView <NSDraggingSource>
@property(nonatomic, strong) NSURL *bundleURL;
@property(nonatomic, strong) NSImage *appIcon;
@property(nonatomic, copy) NSString *appName;
@property(nonatomic, strong) NSEvent *mouseDownEvent;
@end

/// The name System Settings lists the app under.
static NSString *fractileAppName(NSURL *bundleURL) {
    NSString *name = [[NSFileManager defaultManager] displayNameAtPath:bundleURL.path];
    if ([name.pathExtension.lowercaseString isEqualToString:@"app"]) {
        name = name.stringByDeletingPathExtension;
    }
    return name.length > 0 ? name : @"Fractile";
}

/// macOS 27 renamed the Accessibility list to Device Control and Data Access.
static BOOL accessibilityRenamed(void) {
    NSOperatingSystemVersion version = {27, 0, 0};
    return [NSProcessInfo.processInfo isOperatingSystemAtLeastVersion:version];
}

/// macOS 27.3 renamed the list again to System & App Protection Override and
/// stopped accepting tiles dragged into it; the pane adds apps with its own +
/// control instead. macOS 27.0 through 27.2 still takes the dragged tile.
static BOOL accessibilityProtected(void) {
    NSOperatingSystemVersion version = {27, 3, 0};
    return [NSProcessInfo.processInfo isOperatingSystemAtLeastVersion:version];
}

/// The name of the Privacy & Security list Accessibility lives in.
const char *fractile_accessibility_list_name(void) {
    if (accessibilityProtected()) {
        return "System & App Protection Override";
    }
    return accessibilityRenamed() ? "Device Control and Data Access" : "Accessibility";
}

/// Whether System Settings still accepts Fractile's tile dragged into its list.
bool fractile_accessibility_drag_supported(void) {
    return !accessibilityProtected();
}

/// Asks macOS to register this running binary in Accessibility and show its
/// prompt when untrusted.
bool fractile_prompt_accessibility_trust(void) {
    NSDictionary *options = @{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @YES};
    return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)options);
}

static BOOL isDark(NSAppearance *appearance) {
    NSAppearanceName match = [appearance bestMatchFromAppearancesWithNames:@[
        NSAppearanceNameAqua,
        NSAppearanceNameDarkAqua,
    ]];
    return [match isEqualToString:NSAppearanceNameDarkAqua];
}

/// The app's tile as System Settings will list it: icon and name.
static void drawTile(NSRect bounds, NSImage *icon, NSString *name, BOOL dark) {
    NSBezierPath *background = [NSBezierPath bezierPathWithRoundedRect:bounds
                                                               xRadius:8
                                                               yRadius:8];
    [[NSColor colorWithWhite:(dark ? 0.18 : 0.89) alpha:1] setFill];
    [background fill];
    [[NSColor colorWithWhite:(dark ? 1 : 0) alpha:0.10] setStroke];
    background.lineWidth = 1;
    [background stroke];

    [icon drawInRect:NSMakeRect(10, 6, 32, 32)];
    NSDictionary *attributes = @{
        NSFontAttributeName: [NSFont systemFontOfSize:13 weight:NSFontWeightSemibold],
        NSForegroundColorAttributeName: [NSColor colorWithWhite:(dark ? 0.94 : 0.12) alpha:1],
    };
    [name drawAtPoint:NSMakePoint(52, 14) withAttributes:attributes];
}

@implementation FractileDragView

- (instancetype)initWithFrame:(NSRect)frame bundleURL:(NSURL *)bundleURL {
    self = [super initWithFrame:frame];
    if (self) {
        _bundleURL = bundleURL;
        _appIcon = [[NSWorkspace sharedWorkspace] iconForFile:bundleURL.path];
        _appIcon.size = NSMakeSize(32, 32);
        _appName = fractileAppName(bundleURL);
    }
    return self;
}

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return NO;
}

- (void)resetCursorRects {
    [self addCursorRect:self.bounds cursor:NSCursor.openHandCursor];
}

- (void)drawRect:(NSRect)dirtyRect {
    [super drawRect:dirtyRect];
    drawTile(self.bounds, self.appIcon, self.appName, isDark(self.effectiveAppearance));
}

- (void)mouseDown:(NSEvent *)event {
    self.mouseDownEvent = event;
}

- (void)mouseDragged:(NSEvent *)event {
    if (self.mouseDownEvent == nil) {
        return;
    }
    CGFloat dx = event.locationInWindow.x - self.mouseDownEvent.locationInWindow.x;
    CGFloat dy = event.locationInWindow.y - self.mouseDownEvent.locationInWindow.y;
    if (hypot(dx, dy) < 3) {
        return;
    }

    NSDraggingItem *item = [[NSDraggingItem alloc] initWithPasteboardWriter:self.bundleURL];
    NSBitmapImageRep *representation = [self bitmapImageRepForCachingDisplayInRect:self.bounds];
    [self cacheDisplayInRect:self.bounds toBitmapImageRep:representation];
    NSImage *snapshot = [[NSImage alloc] initWithSize:self.bounds.size];
    [snapshot addRepresentation:representation];
    [item setDraggingFrame:self.bounds contents:snapshot];

    NSDraggingSession *session = [self beginDraggingSessionWithItems:@[ item ]
                                                               event:self.mouseDownEvent
                                                              source:self];
    session.animatesToStartingPositionsOnCancelOrFail = YES;
    session.draggingFormation = NSDraggingFormationNone;
    self.mouseDownEvent = nil;
}

- (NSDragOperation)draggingSession:(NSDraggingSession *)session
        sourceOperationMaskForDraggingContext:(NSDraggingContext)context {
    return NSDragOperationCopy;
}

@end

@interface FractileGuideContentView : NSView
@end

@implementation FractileGuideContentView

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return YES;
}

@end

@interface FractileGuideLabel : NSTextField
@end

@implementation FractileGuideLabel

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return YES;
}

- (void)mouseDown:(NSEvent *)event {
    [self.window performWindowDragWithEvent:event];
}

@end

@interface FractileCloseButton : NSButton
@end

@implementation FractileCloseButton

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    (void)event;
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return NO;
}

- (void)resetCursorRects {
    [self addCursorRect:self.bounds cursor:NSCursor.pointingHandCursor];
}

@end

static void closeGuide(void);

@interface FractileGuideDelegate : NSObject <NSWindowDelegate>
@end

@implementation FractileGuideDelegate

- (void)windowDidMove:(NSNotification *)notification {
    (void)notification;
    if (!guidePlacingProgrammatically) {
        guideUserMoved = YES;
    }
}

- (void)dismissGuide:(id)sender {
    (void)sender;
    closeGuide();
}

@end

static double clampDouble(double value, double minValue, double maxValue) {
    if (minValue > maxValue) {
        return minValue;
    }
    return MIN(MAX(value, minValue), maxValue);
}

static bool rectsIntersect(FractileGuideRect a, FractileGuideRect b) {
    return a.x < b.x + b.width &&
           a.x + a.width > b.x &&
           a.y < b.y + b.height &&
           a.y + a.height > b.y;
}

/// Computes the AppKit origin (bottom-left) for the floating permission guide.
///
/// On macOS 27.3+ (`protected_mode == true`), confirmation buttons sit along the
/// bottom of System Settings and its modal sheets. Prefer placing the bar above
/// Settings, else below Settings, else near the top of `visible` — never fall
/// back to the bottom of `visible` over those buttons.
FractileGuidePoint fractile_permission_guide_origin(
    FractileGuideRect settings,
    FractileGuideRect visible,
    FractileGuideSize panel,
    bool protected_mode
) {
    const double edgeMargin = 8.0;
    const double protectedGap = 12.0;
    const double protectedTopFallbackMargin = 12.0;
    const double bottomAvoidHeight = 120.0;

    double minX = visible.x + edgeMargin;
    double maxX = visible.x + visible.width - panel.width - edgeMargin;
    double minY = visible.y + edgeMargin;
    double maxY = visible.y + visible.height - panel.height - edgeMargin;

    if (!protected_mode) {
        double x = (settings.x + settings.width) - panel.width - 16.0;
        double y = settings.y + 16.0;
        return (FractileGuidePoint){
            .x = clampDouble(x, minX, maxX),
            .y = clampDouble(y, minY, maxY),
        };
    }

    double settingsCenterX = settings.x + settings.width / 2.0;
    double x = clampDouble(settingsCenterX - panel.width / 2.0, minX, maxX);

    double aboveY = (settings.y + settings.height) + protectedGap;
    double belowY = settings.y - panel.height - protectedGap;
    double topFallbackY = visible.y + visible.height - panel.height - protectedTopFallbackMargin;

    double y;
    if (aboveY >= minY && aboveY <= maxY) {
        y = aboveY;
    } else if (belowY >= minY && belowY <= maxY) {
        y = belowY;
    } else {
        y = clampDouble(topFallbackY, minY, maxY);
        double dangerTop = settings.y + MIN(bottomAvoidHeight, settings.height);
        if (y < dangerTop && maxY > y) {
            y = maxY;
        }
    }

    return (FractileGuidePoint){ .x = x, .y = y };
}

/// Inspects a front-to-back on-screen window list to locate System Settings'
/// main window and detect any modal sheet, dialog, or system security/TCC
/// approval prompt that must never be overlapped by the floating guide.
FractileGuideWindowAnalysis fractile_permission_guide_analyze_windows(
    const FractileGuideWindowInfo *windows,
    size_t count,
    int32_t self_pid,
    int32_t modal_layer
) {
    FractileGuideWindowAnalysis result = {
        .has_settings_frame = false,
        .settings_frame = {0, 0, 0, 0},
        .has_blocking_sheet_or_prompt = false,
    };
    if (windows == NULL || count == 0) {
        return result;
    }

    size_t settingsIndex = count;
    double bestArea = 0.0;
    for (size_t i = 0; i < count; i++) {
        FractileGuideWindowInfo w = windows[i];
        if (!w.is_settings_owner || w.layer != 0 || w.alpha <= 0.01) {
            continue;
        }
        if (w.frame.width < 300.0 || w.frame.height < 200.0) {
            continue;
        }
        double area = w.frame.width * w.frame.height;
        if (!result.has_settings_frame || area > bestArea) {
            result.has_settings_frame = true;
            result.settings_frame = w.frame;
            settingsIndex = i;
            bestArea = area;
        }
    }

    size_t settingsVisibleWindows = 0;
    for (size_t i = 0; i < count; i++) {
        FractileGuideWindowInfo w = windows[i];
        if (w.alpha <= 0.01 || w.frame.width < 100.0 || w.frame.height < 60.0) {
            continue;
        }
        if (w.is_settings_owner) {
            if (w.layer >= 0 && w.layer < 24) {
                settingsVisibleWindows += 1;
                if (w.layer > 0) {
                    result.has_blocking_sheet_or_prompt = true;
                }
            }
            continue;
        }
        if (w.is_system_prompt_owner && w.layer >= 0) {
            result.has_blocking_sheet_or_prompt = true;
            continue;
        }
        if (!result.has_settings_frame || w.is_ignored_chrome || w.pid == self_pid) {
            continue;
        }
        if (rectsIntersect(w.frame, result.settings_frame)) {
            if (w.layer == modal_layer || (w.layer == 0 && i < settingsIndex)) {
                result.has_blocking_sheet_or_prompt = true;
            }
        }
    }

    if (settingsVisibleWindows > 1) {
        result.has_blocking_sheet_or_prompt = true;
    }

    return result;
}

static BOOL isSystemPromptOwner(NSString *owner) {
    return [owner isEqualToString:@"SecurityAgent"] ||
           [owner isEqualToString:@"CoreServicesUIAgent"] ||
           [owner isEqualToString:@"LocalAuthenticationUIAgent"] ||
           [owner isEqualToString:@"UniversalAccessAuthWarn"] ||
           [owner isEqualToString:@"Open and Save Panel Service"] ||
           [owner isEqualToString:@"UserNotificationCenter"];
}

static BOOL isIgnoredChromeOwner(NSString *owner) {
    return [owner isEqualToString:@"Window Server"] ||
           [owner isEqualToString:@"Dock"] ||
           [owner isEqualToString:@"Control Center"] ||
           [owner isEqualToString:@"SystemUIServer"] ||
           [owner isEqualToString:@"Notification Center"];
}

static FractileGuideWindowAnalysis inspectSettingsWindows(void) {
    NSArray<NSRunningApplication *> *settingsApps =
        [NSRunningApplication runningApplicationsWithBundleIdentifier:settingsBundleID];
    pid_t settingsPID = settingsApps.count > 0 ? settingsApps.firstObject.processIdentifier : 0;

    CFArrayRef windowInfo = CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID
    );
    NSArray *windows = CFBridgingRelease(windowInfo);
    size_t count = (size_t)windows.count;
    if (count == 0) {
        return fractile_permission_guide_analyze_windows(NULL, 0, (int32_t)getpid(), 8);
    }

    FractileGuideWindowInfo *entries = calloc(count, sizeof(FractileGuideWindowInfo));
    if (entries == NULL) {
        return fractile_permission_guide_analyze_windows(NULL, 0, (int32_t)getpid(), 8);
    }

    size_t validCount = 0;
    for (NSDictionary *window in windows) {
        CGRect frame;
        if (!CGRectMakeWithDictionaryRepresentation(
                (__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &frame)) {
            continue;
        }
        NSString *owner = window[(id)kCGWindowOwnerName];
        int32_t pid = (int32_t)[window[(id)kCGWindowOwnerPID] intValue];
        int32_t layer = (int32_t)[window[(id)kCGWindowLayer] integerValue];
        NSNumber *alphaValue = window[(id)kCGWindowAlpha];
        double alpha = alphaValue != nil ? alphaValue.doubleValue : 1.0;

        BOOL isSettingsOwner = (settingsPID > 0 && pid == (int32_t)settingsPID) ||
                               [owner isEqualToString:@"System Settings"] ||
                               [owner isEqualToString:@"System Preferences"];

        entries[validCount++] = (FractileGuideWindowInfo){
            .pid = pid,
            .layer = layer,
            .alpha = alpha,
            .frame = {
                .x = frame.origin.x,
                .y = frame.origin.y,
                .width = frame.size.width,
                .height = frame.size.height,
            },
            .is_settings_owner = isSettingsOwner,
            .is_system_prompt_owner = isSystemPromptOwner(owner),
            .is_ignored_chrome = isIgnoredChromeOwner(owner),
        };
    }

    int32_t modalLayer = (int32_t)CGWindowLevelForKey(kCGModalPanelWindowLevelKey);
    FractileGuideWindowAnalysis analysis = fractile_permission_guide_analyze_windows(
        entries,
        validCount,
        (int32_t)getpid(),
        modalLayer
    );
    free(entries);
    return analysis;
}

/// Whether System Settings is the application in front, so the guide has
/// somewhere to point and must not float over anything else.
static BOOL settingsFrontmost(void) {
    NSRunningApplication *front = NSWorkspace.sharedWorkspace.frontmostApplication;
    return [front.bundleIdentifier isEqualToString:settingsBundleID];
}

static BOOL settingsHasWindow(void) {
    NSArray<NSRunningApplication *> *apps =
        [NSRunningApplication runningApplicationsWithBundleIdentifier:settingsBundleID];
    if (apps.count == 0) {
        return NO;
    }
    pid_t pid = apps.firstObject.processIdentifier;
    NSArray *windows = CFBridgingRelease(CGWindowListCopyWindowInfo(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, kCGNullWindowID));
    for (NSDictionary *window in windows) {
        if ([window[(id)kCGWindowOwnerPID] intValue] == pid &&
            [window[(id)kCGWindowLayer] integerValue] == 0) {
            return YES;
        }
    }
    return NO;
}

/// Bit 0: System Settings has an on-screen window.
/// Bit 1: System Settings is the frontmost application.
/// Bit 2: Fractile is the active application.
int fractile_permission_settings_state(void) {
    NSCAssert(NSThread.isMainThread, @"settings state must run on the main thread");
    BOOL open = settingsHasWindow();
    BOOL frontmost = settingsFrontmost();
    BOOL active = NSApp.isActive;
    return (open ? 1 : 0) | (frontmost ? 2 : 0) | (active ? 4 : 0);
}

/// Whether System Settings still has a real window anywhere, on another Space
/// or minimized included, rather than having been closed.
static BOOL settingsWindowExists(void) {
    NSArray<NSRunningApplication *> *apps =
        [NSRunningApplication runningApplicationsWithBundleIdentifier:settingsBundleID];
    if (apps.count == 0) {
        return NO;
    }
    pid_t pid = apps.firstObject.processIdentifier;
    NSArray *windows = CFBridgingRelease(
        CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID));
    for (NSDictionary *window in windows) {
        if ([window[(id)kCGWindowOwnerPID] intValue] != pid ||
            [window[(id)kCGWindowLayer] integerValue] != 0) {
            continue;
        }
        CGRect frame;
        if (CGRectMakeWithDictionaryRepresentation(
                (__bridge CFDictionaryRef)window[(id)kCGWindowBounds], &frame) &&
            frame.size.width >= 300 && frame.size.height >= 200) {
            return YES;
        }
    }
    return NO;
}

/// Opens System Settings at `url`. A System Settings that isn't running yet
/// drops the pane in the URL while it restores the pane it showed last, so
/// after a cold launch the URL goes again once its window is up and settled.
static void openSettingsPaneMayQuit(NSURL *url, BOOL mayQuit) {
    NSArray<NSRunningApplication *> *running =
        [NSRunningApplication runningApplicationsWithBundleIdentifier:settingsBundleID];
    BOOL cold = running.count == 0;
    NSUInteger request = ++settingsRequest;
    // A Settings window left on another Space would pull the whole screen
    // there, away from Fractile. Quit it so it opens fresh on this Space.
    if (mayQuit && !cold && !settingsHasWindow() && settingsWindowExists()) {
        NSRunningApplication *settings = running.firstObject;
        [settings terminate];
        __block NSUInteger waits = 0;
        [NSTimer scheduledTimerWithTimeInterval:0.05 repeats:YES block:^(NSTimer *timer) {
            waits += 1;
            if (request != settingsRequest) {
                [timer invalidate];
                return;
            }
            if (!settings.terminated && waits < 40) {
                return;
            }
            [timer invalidate];
            openSettingsPaneMayQuit(url, NO);
        }];
        return;
    }
    [[NSWorkspace sharedWorkspace] openURL:url];
    if (!cold) {
        return;
    }
    __block NSUInteger polls = 0;
    [NSTimer scheduledTimerWithTimeInterval:0.1 repeats:YES block:^(NSTimer *timer) {
        polls += 1;
        if (request != settingsRequest || polls > 100) {
            [timer invalidate];
            return;
        }
        if (!settingsHasWindow()) {
            return;
        }
        [timer invalidate];
        for (NSNumber *delay in @[ @0.35, @1.2 ]) {
            dispatch_after(
                dispatch_time(DISPATCH_TIME_NOW, (int64_t)(delay.doubleValue * NSEC_PER_SEC)),
                dispatch_get_main_queue(),
                ^{
                    if (request == settingsRequest) {
                        [[NSWorkspace sharedWorkspace] openURL:url];
                    }
                }
            );
        }
    }];
}

static void openSettingsPane(NSURL *url) {
    openSettingsPaneMayQuit(url, YES);
}

static NSRect appKitFrame(CGRect frame) {
    NSScreen *primary = NSScreen.screens.firstObject;
    if (primary == nil) {
        return NSRectFromCGRect(frame);
    }
    return NSMakeRect(
        frame.origin.x,
        primary.frame.size.height - frame.origin.y - frame.size.height,
        frame.size.width,
        frame.size.height
    );
}

static void endFlight(void) {
    [flightTimer invalidate];
    flightTimer = nil;
    [flightPanel orderOut:nil];
    flightPanel = nil;
    flightLayer = nil;
    guideTile.hidden = NO;
    guidePanel.alphaValue = 1;
}

static void closeGuide(void) {
    settingsRequest += 1;
    endFlight();
    [guideTimer invalidate];
    guideTimer = nil;
    guidePanel.delegate = nil;
    [guidePanel orderOut:nil];
    guidePanel = nil;
    guideDelegate = nil;
    guideTile = nil;
    guideSettingsURL = nil;
    sawSettingsWindow = NO;
    guideUserMoved = NO;
    guidePlacingProgrammatically = NO;
}

/// Whether the Accessibility guide is open, in front or waiting behind another
/// app for System Settings to come back.
bool fractile_accessibility_guide_active(void) {
    NSCAssert(NSThread.isMainThread, @"permission guide must run on the main thread");
    return guidePanel != nil;
}

/// Dismisses the Accessibility guide and cancels any pending cold-launch retry.
void fractile_close_accessibility_guide(void) {
    NSCAssert(NSThread.isMainThread, @"permission guide must run on the main thread");
    closeGuide();
}

static CGFloat clamp01(CGFloat value) {
    return MIN(MAX(value, 0), 1);
}

static CGFloat easeInOutCubic(CGFloat t) {
    return t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2;
}

static CGFloat easeOutCubic(CGFloat t) {
    return 1 - pow(1 - t, 3);
}

/// Where the tile lands: its slot in the guide, in screen coordinates.
static NSPoint tileTarget(void) {
    NSRect slot = [guideTile convertRect:guideTile.bounds toView:nil];
    NSRect screen = [guidePanel convertRectToScreen:slot];
    return NSMakePoint(NSMidX(screen), NSMidY(screen));
}

static void placeFlight(NSPoint center, CGFloat scale, CGFloat blur, CGFloat opacity) {
    NSSize size = flightPanel.frame.size;
    [flightPanel setFrameOrigin:NSMakePoint(center.x - size.width / 2, center.y - size.height / 2)];
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    flightLayer.transform = CATransform3DMakeScale(scale, scale, 1);
    flightLayer.opacity = (float)opacity;
    [flightLayer setValue:@(blur) forKeyPath:@"filters.blur.inputRadius"];
    [CATransaction commit];
}

static void stepFlight(void) {
    CFTimeInterval now = CACurrentMediaTime();
    if (flightStartedAt == 0) {
        // Lifted out of the row, waiting for System Settings to open.
        if (now - flightShownAt > flightPatience) {
            endFlight();
            return;
        }
        CGFloat t = easeOutCubic(clamp01((now - flightShownAt) / flightLift));
        placeFlight(NSMakePoint(flightOrigin.x, flightOrigin.y + 12 * t), 0.7 + 0.1 * t,
                    4 * (1 - t), t);
        return;
    }
    CGFloat t = clamp01((now - flightStartedAt) / flightDuration);
    CGFloat e = easeInOutCubic(t);
    NSPoint start = NSMakePoint(flightOrigin.x, flightOrigin.y + 12);
    NSPoint end = tileTarget();
    CGFloat distance = hypot(end.x - start.x, end.y - start.y);
    NSPoint control = NSMakePoint((start.x + end.x) / 2,
                                  MAX(start.y, end.y) + MIN(160, distance * 0.25));
    CGFloat u = 1 - e;
    NSPoint point = NSMakePoint(u * u * start.x + 2 * u * e * control.x + e * e * end.x,
                                u * u * start.y + 2 * u * e * control.y + e * e * end.y);
    placeFlight(point, 0.8 + 0.2 * e, 6 * sin(M_PI * e), 1);
    guidePanel.alphaValue = easeOutCubic(clamp01((t - 0.45) / 0.55));
    if (t >= 1) {
        endFlight();
    }
}

/// Whether the click that asked for the guide was inside one of Fractile's windows.
static BOOL clickInFractileWindow(NSPoint point) {
    for (NSWindow *window in NSApp.windows) {
        if (window.visible && window != guidePanel && window != flightPanel &&
            NSPointInRect(point, window.frame)) {
            return YES;
        }
    }
    return NO;
}

/// Lifts a copy of the tile out of Fractile's window at `origin`.
static void beginFlight(NSPoint origin, NSURL *bundleURL, NSSize tileSize) {
    flightOrigin = origin;
    flightShownAt = CACurrentMediaTime();
    flightStartedAt = 0;
    NSImage *icon = [[NSWorkspace sharedWorkspace] iconForFile:bundleURL.path];
    icon.size = NSMakeSize(32, 32);
    NSString *name = fractileAppName(bundleURL);
    BOOL dark = isDark(NSApp.effectiveAppearance);
    NSImage *image = [NSImage imageWithSize:tileSize
                                    flipped:NO
                             drawingHandler:^BOOL(NSRect rect) {
        drawTile(rect, icon, name, dark);
        return YES;
    }];

    NSSize size = NSMakeSize(tileSize.width + 2 * flightPadding, tileSize.height + 2 * flightPadding);
    flightPanel = [[NSPanel alloc] initWithContentRect:NSMakeRect(0, 0, size.width, size.height)
                                             styleMask:NSWindowStyleMaskBorderless |
                                                       NSWindowStyleMaskNonactivatingPanel
                                               backing:NSBackingStoreBuffered
                                                 defer:NO];
    flightPanel.level = NSFloatingWindowLevel + 1;
    flightPanel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                     NSWindowCollectionBehaviorFullScreenAuxiliary;
    flightPanel.opaque = NO;
    flightPanel.backgroundColor = NSColor.clearColor;
    flightPanel.hasShadow = NO;
    flightPanel.ignoresMouseEvents = YES;
    flightPanel.hidesOnDeactivate = NO;
    flightPanel.releasedWhenClosed = NO;

    NSView *host = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, size.width, size.height)];
    host.layer = [CALayer layer];
    host.wantsLayer = YES;
    host.layerUsesCoreImageFilters = YES;
    flightLayer = [CALayer layer];
    flightLayer.bounds = CGRectMake(0, 0, tileSize.width, tileSize.height);
    flightLayer.position = CGPointMake(size.width / 2, size.height / 2);
    flightLayer.contents = image;
    CGFloat backingScale = NSScreen.mainScreen != nil ? NSScreen.mainScreen.backingScaleFactor : 2.0;
    flightLayer.contentsScale = backingScale > 0 ? backingScale : 1.0;
    flightLayer.shadowOpacity = 0.25;
    flightLayer.shadowRadius = 12;
    flightLayer.shadowOffset = CGSizeMake(0, -6);
    CIFilter *blur = [CIFilter filterWithName:@"CIGaussianBlur"];
    blur.name = @"blur";
    [blur setValue:@0 forKey:kCIInputRadiusKey];
    flightLayer.filters = @[ blur ];
    [host.layer addSublayer:flightLayer];
    flightPanel.contentView = host;

    placeFlight(origin, 0.7, 4, 0);
    [flightPanel orderFrontRegardless];
    flightTimer = [NSTimer timerWithTimeInterval:1.0 / 120 repeats:YES block:^(NSTimer *timer) {
        (void)timer;
        stepFlight();
    }];
    [[NSRunLoop mainRunLoop] addTimer:flightTimer forMode:NSRunLoopCommonModes];
}

static void updateGuidePosition(void) {
    if (AXIsProcessTrusted()) {
        closeGuide();
        return;
    }
    FractileGuideWindowAnalysis analysis = inspectSettingsWindows();
    if (!analysis.has_settings_frame) {
        if (!sawSettingsWindow) {
            return;
        }
        // Settings on another Space or minimized: wait for it, out of sight.
        [guidePanel orderOut:nil];
        if (settingsWindowExists()) {
            return;
        }
        // Closed with Accessibility still missing: bring the pane back rather
        // than leave the guide pointing at nothing.
        if (guideSettingsURL == nil || guideReopens >= guideReopenLimit) {
            closeGuide();
            return;
        }
        guideReopens += 1;
        sawSettingsWindow = NO;
        openSettingsPane(guideSettingsURL);
        return;
    }
    sawSettingsWindow = YES;
    if (!settingsFrontmost() || analysis.has_blocking_sheet_or_prompt) {
        // Another app is in front, or System Settings has a modal sheet /
        // system approval prompt open: stay out of sight so the guide never
        // blocks a confirmation button, and return once it clears.
        if (analysis.has_blocking_sheet_or_prompt && flightPanel != nil) {
            endFlight();
        }
        [guidePanel orderOut:nil];
        return;
    }
    CGRect quartzFrame = CGRectMake(
        analysis.settings_frame.x,
        analysis.settings_frame.y,
        analysis.settings_frame.width,
        analysis.settings_frame.height
    );
    NSRect settings = appKitFrame(quartzFrame);
    NSSize size = guidePanel.frame.size;
    NSScreen *screen = nil;
    for (NSScreen *candidate in NSScreen.screens) {
        if (NSIntersectsRect(candidate.frame, settings)) {
            screen = candidate;
            break;
        }
    }
    if (screen == nil) {
        screen = NSScreen.mainScreen;
    }
    NSRect visible = screen != nil ? screen.visibleFrame : settings;
    if (!guideUserMoved) {
        FractileGuidePoint origin = fractile_permission_guide_origin(
            (FractileGuideRect){
                .x = settings.origin.x,
                .y = settings.origin.y,
                .width = settings.size.width,
                .height = settings.size.height,
            },
            (FractileGuideRect){
                .x = visible.origin.x,
                .y = visible.origin.y,
                .width = visible.size.width,
                .height = visible.size.height,
            },
            (FractileGuideSize){
                .width = size.width,
                .height = size.height,
            },
            accessibilityProtected()
        );
        guidePlacingProgrammatically = YES;
        [guidePanel setFrameOrigin:NSMakePoint(origin.x, origin.y)];
        guidePlacingProgrammatically = NO;
    }
    if (flightPanel != nil && flightStartedAt == 0) {
        flightStartedAt = CACurrentMediaTime();
    }
    if (flightPanel != nil) {
        [flightPanel orderFrontRegardless];
    }
    guidePlacingProgrammatically = YES;
    [guidePanel orderFrontRegardless];
    guidePlacingProgrammatically = NO;
}

void fractile_show_accessibility_guide(void) {
    NSCAssert(NSThread.isMainThread, @"permission guide must run on the main thread");
    NSPoint click = NSEvent.mouseLocation;
    // macOS 27.3 rejects dragged tiles, so it gets a bar with click instructions
    // instead of the drag tile and its flight. macOS 27.0 through 27.2 keeps the
    // drag guide and its old list name.
    BOOL renamed = accessibilityProtected();
    BOOL fly = !renamed && clickInFractileWindow(click) &&
               !NSWorkspace.sharedWorkspace.accessibilityDisplayShouldReduceMotion;
    closeGuide();
    guideReopens = 0;

    NSURL *bundleURL = NSBundle.mainBundle.bundleURL.standardizedURL;
    NSString *listName = [NSString stringWithUTF8String:fractile_accessibility_list_name()];
    NSString *appName = fractileAppName(bundleURL);
    NSURL *settingsURL = [NSURL URLWithString:
        @"x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility"];
    openSettingsPane(settingsURL);
    guideSettingsURL = settingsURL;
    // A development binary has no bundle to list, so it gets the pane alone.
    if (![bundleURL.pathExtension.lowercaseString isEqualToString:@"app"]) {
        return;
    }

    NSFont *instructionFont = [NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
    NSString *text;
    NSSize panelSize;
    NSScreen *mainScreen = NSScreen.mainScreen;
    if (renamed) {
        text = [NSString stringWithFormat:
            @"Click the Privacy & Security notification, or add %@ in %@",
            appName, listName];
        CGFloat width = ceil([text sizeWithAttributes:@{ NSFontAttributeName: instructionFont }].width) + 66;
        if (mainScreen != nil) {
            width = MIN(width, mainScreen.visibleFrame.size.width - 32);
        }
        panelSize = NSMakeSize(MAX(width, 320), 52);
    } else {
        text = [NSString stringWithFormat:@"↑  Drag %@ into the %@ list above", appName, listName];
        CGFloat width = MAX(480, ceil([text sizeWithAttributes:@{ NSFontAttributeName: instructionFont }].width) + 66);
        if (mainScreen != nil) {
            width = MIN(width, mainScreen.visibleFrame.size.width - 32);
        }
        panelSize = NSMakeSize(width, 116);
    }
    guidePanel = [[NSPanel alloc]
        initWithContentRect:NSMakeRect(0, 0, panelSize.width, panelSize.height)
                  styleMask:NSWindowStyleMaskBorderless |
                            NSWindowStyleMaskNonactivatingPanel |
                            NSWindowStyleMaskFullSizeContentView
                    backing:NSBackingStoreBuffered
                      defer:NO];
    guideDelegate = [[FractileGuideDelegate alloc] init];
    guidePanel.delegate = guideDelegate;
    guidePanel.floatingPanel = YES;
    guidePanel.level = NSFloatingWindowLevel;
    guidePanel.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                    NSWindowCollectionBehaviorFullScreenAuxiliary;
    guidePanel.opaque = NO;
    guidePanel.backgroundColor = NSColor.clearColor;
    guidePanel.hasShadow = YES;
    guidePanel.hidesOnDeactivate = NO;
    guidePanel.releasedWhenClosed = NO;
    guidePanel.movableByWindowBackground = YES;

    FractileGuideContentView *content =
        [[FractileGuideContentView alloc] initWithFrame:NSMakeRect(0, 0, panelSize.width, panelSize.height)];
    content.wantsLayer = YES;
    content.layer.backgroundColor = NSColor.controlBackgroundColor.CGColor;
    content.layer.cornerRadius = renamed ? 14 : 20;
    content.layer.borderWidth = 1;
    content.layer.borderColor = [NSColor colorWithWhite:0 alpha:0.10].CGColor;

    FractileGuideLabel *instruction = [FractileGuideLabel labelWithString:text];
    instruction.frame = renamed
        ? NSMakeRect(18, (panelSize.height - 18) / 2, panelSize.width - 66, 18)
        : NSMakeRect(18, 76, panelSize.width - 66, 18);
    instruction.font = instructionFont;
    instruction.textColor = NSColor.labelColor;
    instruction.lineBreakMode = NSLineBreakByTruncatingTail;
    [content addSubview:instruction];

    FractileCloseButton *closeButton = [[FractileCloseButton alloc]
        initWithFrame:renamed
            ? NSMakeRect(panelSize.width - 36, (panelSize.height - 24) / 2, 24, 24)
            : NSMakeRect(panelSize.width - 36, 73, 24, 24)];
    closeButton.bordered = NO;
    [closeButton setButtonType:NSButtonTypeMomentaryPushIn];
    NSImage *xmark = [NSImage imageWithSystemSymbolName:@"xmark"
                               accessibilityDescription:@"Dismiss"];
    if (xmark != nil) {
        NSImageSymbolConfiguration *config =
            [NSImageSymbolConfiguration configurationWithPointSize:11
                                                            weight:NSFontWeightSemibold];
        closeButton.image = [xmark imageWithSymbolConfiguration:config];
        closeButton.imagePosition = NSImageOnly;
        closeButton.contentTintColor = NSColor.secondaryLabelColor;
    } else {
        closeButton.title = @"×";
        closeButton.font = [NSFont systemFontOfSize:14 weight:NSFontWeightMedium];
    }
    closeButton.toolTip = @"Dismiss";
    closeButton.target = guideDelegate;
    closeButton.action = @selector(dismissGuide:);
    [content addSubview:closeButton];

    if (!renamed) {
        FractileDragView *dragView = [[FractileDragView alloc]
            initWithFrame:NSMakeRect(18, 17, panelSize.width - 36, 44)
                 bundleURL:bundleURL];
        [content addSubview:dragView];
        guideTile = dragView;
    }
    guidePanel.contentView = content;

    if (fly) {
        guideTile.hidden = YES;
        guidePanel.alphaValue = 0;
        beginFlight(click, bundleURL, guideTile.frame.size);
    }

    guideTimer = [NSTimer scheduledTimerWithTimeInterval:0.12
                                                 repeats:YES
                                                   block:^(NSTimer *timer) {
        (void)timer;
        updateGuidePosition();
    }];
    updateGuidePosition();
}
