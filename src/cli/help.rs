pub(crate) fn version() -> String {
    format!("fractile {}", fractile::build_version())
}

pub(crate) fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "-h" | "--help")
}

pub(crate) fn root_usage(executable: &str) -> String {
    let mut lines = vec![
        format!("usage: {executable} <command> [args...]"),
        format!("       {executable} --help | --version"),
        String::new(),
        "Configuration commands:".into(),
        format!("  {executable} config <path|show|get|set|check> ..."),
        format!("  {executable} rules <list|add|remove> ..."),
        format!("  {executable} workspaces <list|assign|clear> ..."),
        format!("  {executable} config-check <config.toml>"),
    ];
    #[cfg(target_os = "macos")]
    {
        lines.extend([
            String::new(),
            "Runtime control & diagnostics:".into(),
            format!("  {executable} ctl <command> [args...]"),
            format!("  {executable} open <url-or-target>"),
            format!("  {executable} displays"),
            format!("  {executable} snapshot"),
            format!("  {executable} hud"),
            format!("  {executable} retile [--apply]"),
            format!("  {executable} watch <cycles> [--apply]"),
            format!("  {executable} watch-window <window-id> <cycles> [--apply]"),
            format!("  {executable} run [--apply]"),
        ]);
    }
    lines.extend([
        String::new(),
        "Inspection & fixtures:".into(),
        format!("  {executable} doctor [hours]"),
        format!("  {executable} replay <fixture.json>"),
    ]);
    lines.join("\n")
}

pub(crate) fn root(executable: &str) -> String {
    let mut sections = vec![
        format!(
            "Fractile {} — macOS tiling window manager",
            fractile::build_version()
        ),
        String::new(),
        format!("Usage: {executable} <command> [args...]"),
        String::new(),
        "Options:".into(),
        "  -h, --help                 Show help for fractile or a subcommand".into(),
        "  -V, --version              Print the fractile version".into(),
        String::new(),
        "Configuration:".into(),
        "  config <subcommand>        Inspect, validate, and edit config.toml atomically".into(),
        "  rules <subcommand>         List, add, and remove ordered window rules".into(),
        "  workspaces <subcommand>    List, assign, and clear workspace monitor routes".into(),
        "  config-check <path>        Validate a config.toml file".into(),
    ];
    #[cfg(target_os = "macos")]
    {
        sections.extend([
            String::new(),
            "Live Agent Control:".into(),
            "  ctl <command> [args...]    Send a command to the running Fractile agent".into(),
            "  open <url-or-target>       Open Settings to a fractile:// deep link or pane".into(),
            "  displays                   List connected displays as JSON".into(),
            "  snapshot                   Capture a dry-run window observation as JSON".into(),
            "  hud                        Stream live workspace state in the terminal".into(),
            "  retile [--apply]           Run one reconciliation cycle (dry-run by default)".into(),
            "  watch <cycles> [--apply]   Run N reconciliation cycles".into(),
            "  watch-window <id> <cycles> [--apply]".into(),
            "                             Watch a single window ID for N cycles".into(),
            "  run [--apply]              Run the foreground reconciliation loop".into(),
        ]);
    }
    sections.extend([
        String::new(),
        "Diagnostics & Testing:".into(),
        "  doctor [hours]             Summarize the arrangement journal".into(),
        "  replay <fixture.json>      Replay a recorded observation fixture".into(),
        "  help [command]             Show help for fractile or a command".into(),
        String::new(),
        "Environment:".into(),
        "  FRACTILE_CONFIG            Override the path to config.toml".into(),
        "  FRACTILE_APPLICATION_SUPPORT".into(),
        "                             Override the Application Support directory".into(),
        String::new(),
        format!("Run '{executable} <command> --help' for command details."),
    ]);
    sections.join("\n")
}

pub(crate) fn subcommand(executable: &str, name: &str) -> Option<String> {
    let text = match name {
        "config" => config_help(executable),
        "rules" => rules_help(executable),
        "workspaces" => workspaces_help(executable),
        #[cfg(target_os = "macos")]
        "ctl" => ctl_help(executable),
        #[cfg(target_os = "macos")]
        "open" => format!(
            "Open Fractile Settings on a specific pane, feature, or Lab via control.sock.\n\nUsage:\n  {executable} open <url-or-target>\n\nExamples:\n  {executable} open fractile://settings/workspaces\n  {executable} open fractile://labs/scrolling-strip\n  {executable} open fractile://features/gaps\n  {executable} open rules\n\nNotes:\n  - Deep links only open Settings and highlight a target; they never mutate config.toml."
        ),
        "config-check" => format!(
            "Validate a Fractile configuration file.\n\nUsage:\n  {executable} config-check <config.toml>"
        ),
        "doctor" => format!(
            "Summarize the arrangement journal written by the running agent.\n\nUsage:\n  {executable} doctor [hours]\n\nArguments:\n  [hours]    Optional lookback window in hours (positive integer)"
        ),
        "replay" => format!(
            "Replay a recorded observation fixture through the reconciler.\n\nUsage:\n  {executable} replay <fixture.json>"
        ),
        #[cfg(target_os = "macos")]
        "snapshot" => format!(
            "Capture a dry-run window observation and reconciliation as JSON.\n\nUsage:\n  {executable} snapshot"
        ),
        #[cfg(target_os = "macos")]
        "displays" => format!(
            "List connected displays and their IDs, names, and frames as JSON.\n\nUsage:\n  {executable} displays"
        ),
        #[cfg(target_os = "macos")]
        "hud" => format!(
            "Render a live terminal HUD of displays, workspaces, and tiles.\n\nUsage:\n  {executable} hud"
        ),
        #[cfg(target_os = "macos")]
        "retile" => format!(
            "Run one observation and reconciliation cycle.\n\nUsage:\n  {executable} retile [--apply]\n\nOptions:\n  --apply    Apply computed frames to live windows (dry-run by default)"
        ),
        #[cfg(target_os = "macos")]
        "watch" => format!(
            "Run N observation and reconciliation cycles.\n\nUsage:\n  {executable} watch <cycles> [--apply]"
        ),
        #[cfg(target_os = "macos")]
        "watch-window" => format!(
            "Run N cycles scoped to a single window ID.\n\nUsage:\n  {executable} watch-window <window-id> <cycles> [--apply]"
        ),
        #[cfg(target_os = "macos")]
        "run" => format!(
            "Run the persistent reconciliation loop in the foreground.\n\nUsage:\n  {executable} run [--apply]"
        ),
        _ => return None,
    };
    Some(text)
}

pub(crate) fn config_help(executable: &str) -> String {
    [
        "Inspect, validate, and edit Fractile's config.toml atomically.",
        "",
        "Usage:",
        &format!("  {executable} config path [--json]"),
        &format!("  {executable} config show [--json]"),
        &format!("  {executable} config get <key> [--json]"),
        &format!("  {executable} config set <key> <value> [--json]"),
        &format!("  {executable} config reset <key> [--json]"),
        &format!("  {executable} config check [path] [--json]"),
        "",
        "Settings keys:",
        "  hud             true | false      Show the Workspace HUD overlay (default: true)",
        "  hud_style       icons | layouts   Workspace HUD drawing style (default: icons)",
        "  hud_position    bottom | top | left | right  Workspace HUD screen edge (default: bottom)",
        "  hud_stay        true | false      Keep the Workspace HUD on screen; windows tile clear of it (default: false)",
        "  show_in_dock    true | false      Keep Fractile's live icon in the Dock (default: true)",
        "  layout          dwindle | columns How new windows split the layout (default: dwindle)",
        "  mouse_follows_focus  off | display | window  Move pointer on focus changes (default: off)",
        "  trackpad_swipe  true | false      Three-finger swipe switches workspaces (default: true);",
        "                                    read only with labs.swipe_switch on; get/show print the file's value",
        "  menu_bar_icon   full | compact | off  Menu bar icon: a glyph per display, one glyph, or none",
        "                                    (default: full); read only with labs.menu_bar_icon_style on",
        "  pause_on_builtin_display  true | false  Auto-pause on built-in display, resume on external (default: false)",
        "  gaps.inner      0-48              Points between tiled windows (default: 0)",
        "  gaps.outer      0-128             Outer padding at every edge without its own (default: 0)",
        "  gaps.top        0-128             Outer padding at the screen's top edge (default:",
        "  gaps.bottom     0-128               gaps.outer; likewise gaps.bottom, gaps.left, gaps.right)",
        "  gaps.left       0-128",
        "  gaps.right      0-128",
        "  single_window_max_width  0 | 320-16384  Center a lone window at this width (default: 0, off)",
        "  single_window_max_height 0 | 320-16384  Center a lone window at this height (default: 0, off);",
        "                                    read only with labs.single_window_height on",
        "",
        "Labs keys (experimental, off by default):",
        "  labs.scratchpad             true | false  The i3/sway-style Scratchpad and its two shortcuts",
        "  labs.activity               true | false  Settings › Activity: arrangement log and diagnostics",
        "  labs.portrait_edge_rows     true | false  Top/bottom edge drops make rows on portrait displays",
        "  labs.physical_displays      true | false  Menu bar and HUD in physical display order and shape",
        "  labs.recent_focus           true | false  Option+H/J/K/L prefers the last-used bordering tile",
        "  labs.swipe_switch           true | false  An off switch for the three-finger swipe (trackpad_swipe)",
        "  labs.empty_workspace_focus  true | false  Empty workspace chosen on another display is focused",
        "  labs.float_follows_display  true | false  A float dragged to another display joins its workspace",
        "  labs.workspace_steps        true | false  switch next|prev and move next|prev on the focused display",
        "  labs.four_finger_swipe      true | false  The workspace swipe takes four fingers instead of three",
        "  labs.single_window_height   true | false  single_window_max_height caps a lone window's height",
        "  labs.route_all_windows      true | false  windows = \"all\" on a route rule routes every new window",
        "  labs.menu_bar_icon_style    true | false  menu_bar_icon makes the menu bar icon compact or hidden",
        "  labs.focus_follows_mouse    true | false  The tile the pointer rests on takes focus",
        "",
        "Notes:",
        "  - Respects FRACTILE_CONFIG and FRACTILE_APPLICATION_SUPPORT.",
        "  - Every write is validated against version-1 schema and replaced atomically.",
        "  - 'reset' removes a key, and any table it empties, back to its default.",
    ]
    .join("\n")
}

pub(crate) fn rules_help(executable: &str) -> String {
    [
        "Manage ordered, first-match window rules in config.toml.",
        "",
        "Usage:",
        &format!("  {executable} rules list [--json]"),
        &format!(
            "  {executable} rules add --app <bundle-id> (--workspace <ws> | --float | --pin | --tile | --ignore)"
        ),
        "                         [--title <title> | --title-contains <text>] [--windows first|all]",
        "                         [--position <index>] [--json]",
        &format!("  {executable} rules remove <index-or-bundle-id> [--json]"),
        &format!("  {executable} rules remove --app <bundle-id> [--json]"),
        "",
        "Options for 'add':",
        "  --app <bundle-id>          Match windows by macOS bundle identifier (alias: --bundle-id)",
        "  --title <title>            Require an exact window title match",
        "  --title-contains <text>    Require the window title to contain <text>, ignoring case",
        "  --workspace <ws>           Route matching windows to workspace 1-10 or A-Z",
        "  --windows first|all        Which new windows a --workspace rule takes: the app's first (default)",
        "                             or all of them; \"all\" counts only with labs.route_all_windows on",
        "  --float                    Float matching windows without tiling",
        "  --pin                      Float matching windows and show them on every workspace",
        "  --tile                     Tile matching windows even if they look like dialogs",
        "  --ignore                   Ignore matching windows completely",
        "  --position <index>         Insert at 1-based rule index (default: append)",
        "  --json                     Emit machine-readable JSON output",
    ]
    .join("\n")
}

pub(crate) fn workspaces_help(executable: &str) -> String {
    [
        "Manage workspace-to-display monitor assignments in config.toml.",
        "",
        "Usage:",
        &format!("  {executable} workspaces list [--json]"),
        &format!(
            "  {executable} workspaces assign <ws> <main|secondary|tertiary|display-name-or-uuid> [--json]"
        ),
        &format!("  {executable} workspaces clear <ws> [--json]"),
        "",
        "Notes:",
        "  - Workspaces 1-4 default to 'main', 5-7 to 'secondary', 8-10 to 'tertiary'.",
        "  - 'secondary' and 'tertiary' are the non-main displays, left to right. With two",
        "    displays, 'tertiary' workspaces show on the secondary display.",
        "  - Named workspaces A-Z are opt-in and default to 'main'.",
        "  - Use 'fractile displays' to inspect display names and UUIDs.",
    ]
    .join("\n")
}

#[cfg(target_os = "macos")]
pub(crate) fn ctl_help(executable: &str) -> String {
    [
        "Send a live control command to the running Fractile agent over control.sock.",
        "",
        "Usage:",
        &format!("  {executable} ctl <command> [args...]"),
        "",
        "Inspect:",
        "  status                     Print agent status JSON",
        "  debug                      Print live displays, tiles, focus, and targets JSON",
        "  settings                   Print agent-only settings state JSON",
        "",
        "Workspaces:",
        "  switch <workspace>         Switch to workspace 1-10 or A-Z",
        "  back-and-forth             Toggle between the current and previous workspace",
        "  move-workspace [next|prev|<display>]",
        "                             Move the focused workspace to another display (default: next)",
        "  move <workspace> [--follow] Move the focused window to a workspace (and switch with --follow)",
        "  move-tab <workspace> [--follow]",
        "                             Send the focused window's active tab to a workspace as its own window",
        "  move-app <bundle-id> <ws>  Move an application's windows to a workspace",
        "",
        "Windows & layout:",
        "  focus <left|down|up|right|next|prev>",
        "                             Move focus by direction or cycle through the workspace's windows",
        "  move-direction <left|down|up|right>",
        "                             Reorder the focused window in a direction",
        "  layout                     Toggle the workspace split axis",
        "  fullscreen                 Toggle Fullscreen Layout on the active workspace",
        "  float                      Float the focused window above the tiles, or tile it again",
        "  resize <shrink|grow>       Adjust the focused window's layout weight",
        "  resize <width|height> <points>",
        "                             Resize the focused window to that width or height",
        "  balance                    Equalize the focused tile's immediate split",
        "  retile                     Force a fresh observation and retile",
        "",
        "Labs (need [labs] scratchpad = true):",
        "  scratchpad toggle          Summon the next scratchpad window, or hide the one shown",
        "  scratchpad mark            Send the focused window to the scratchpad, or take it out",
        "",
        "Labs (need [labs] workspace_steps = true):",
        "  switch <next|prev>         Show the next or previous occupied workspace of the focused display",
        "  move <next|prev> [--follow]",
        "                             Move the focused window one workspace along its display",
        "",
        "Agent & UI:",
        "  pause | resume             Pause (and restore windows) or resume tiling",
        "  wrong                      Record an anomaly marker in the arrangement journal",
        "  open <url-or-target>       Open Settings on a fractile:// link, pane, feature, or Lab",
        "  open-settings              Open the Fractile Settings window",
        "  open-accessibility         Open Accessibility onboarding / System Settings",
        "  toggle-launch-at-login     Toggle Launch at Login registration",
        "  restart | quit             Restore parked windows and exit the agent (does not relaunch)",
    ]
    .join("\n")
}
