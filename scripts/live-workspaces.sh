#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
baseline_window_ids="$(mktemp)"
baseline_workspaces="$(mktemp)"
test_config="$(mktemp)"
test_state="$(mktemp -d)"
agent_pid=""
test_pid=""
window_id=""
test_title="fractile-workspace-test-$$"
test_script_id=""
test_ax_identifier=""
printf 'version = 1\n' >"$test_config"

cd "$root"
if ! aerospace list-monitors >/dev/null 2>&1; then
  echo "FAIL: AeroSpace must be enabled before running this harness" >&2
  exit 1
fi
if [[ -z "$(aerospace list-workspaces --monitor all --visible)" ]]; then
  echo "FAIL: AeroSpace must have visible workspaces before running this harness" >&2
  exit 1
fi
previous_workspace="$(aerospace list-workspaces --focused)"
previous_monitor="$(aerospace list-monitors --focused --format '%{monitor-id}')"
# NSScreen.screens always lists the main screen first; Fractile uses the other screen as secondary.
test_monitor="$(aerospace list-monitors --format '%{monitor-id}|%{monitor-appkit-nsscreen-screens-id}' | awk -F '|' '$2 != 1 { print $1; exit }')"
previous_test_workspace=""
previous_bundle="$(osascript -e 'tell application "System Events" to get bundle identifier of first application process whose frontmost is true')"
aerospace list-windows --all --format '%{window-id}' | sort >"$baseline_window_ids"
aerospace list-windows --all --format '%{window-id}|%{workspace}' | sort -t '|' -k1,1 >"$baseline_workspaces"

new_window_ids() {
  comm -13 "$baseline_window_ids" <(aerospace list-windows --all --format '%{window-id}' | sort)
}

test_window_ids() {
  [[ -n "$window_id" ]] && printf '%s\n' "$window_id"
}

restore_baseline_workspaces() {
  local current
  current="$(mktemp)"
  aerospace list-windows --all --format '%{window-id}|%{workspace}' | sort -t '|' -k1,1 >"$current"
  join -t '|' -1 1 -2 1 "$baseline_workspaces" "$current" \
    | while IFS='|' read -r window_id baseline_workspace current_workspace; do
        if [[ "$baseline_workspace" != "$current_workspace" ]]; then
          aerospace move-node-to-workspace --window-id "$window_id" "$baseline_workspace" >/dev/null 2>&1
        fi
      done
  rm -f "$current"
}

cleanup() {
  set +e
  if [[ -n "$agent_pid" ]] && kill -0 "$agent_pid" 2>/dev/null; then
    osascript - "$agent_pid" <<'APPLESCRIPT' >/dev/null 2>&1
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events" to tell first application process whose unix id is target_pid
    click UI element 1 of UI element 1
    delay 0.2
    click menu item "Quit Fractile" of menu 1 of UI element 1 of UI element 1
  end tell
end run
APPLESCRIPT
    wait "$agent_pid" 2>/dev/null
  fi
  aerospace enable on >/dev/null 2>&1
  sleep 0.5
  while read -r window_id; do
    [[ -n "$window_id" ]] && aerospace close --window-id "$window_id" >/dev/null 2>&1
  done < <(test_window_ids)
  restore_baseline_workspaces
  if [[ -n "$previous_test_workspace" ]]; then
    aerospace focus-monitor "$test_monitor" >/dev/null 2>&1
    aerospace workspace "$previous_test_workspace" >/dev/null 2>&1
  fi
  aerospace focus-monitor "$previous_monitor" >/dev/null 2>&1
  aerospace workspace "$previous_workspace" >/dev/null 2>&1
  osascript - "$previous_bundle" <<'APPLESCRIPT' >/dev/null 2>&1
on run arguments
  tell application id (item 1 of arguments) to activate
end run
APPLESCRIPT
  rm -rf "$baseline_window_ids" "$baseline_workspaces" "$test_config" "$test_state"
}
trap cleanup EXIT

if [[ ! "$test_monitor" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: a second monitor is required" >&2
  exit 1
fi
previous_test_workspace="$(aerospace list-workspaces --monitor "$test_monitor" --visible)"

aerospace workspace fractile-workspace-test >/dev/null 2>&1
aerospace move-workspace-to-monitor --workspace fractile-workspace-test "$test_monitor" >/dev/null 2>&1
test_script_id="$(osascript - "$test_title" <<'APPLESCRIPT'
on run arguments
  set target_title to item 1 of arguments
  tell application "Ghostty"
    set command_text to "printf '\\033]2;" & target_title & "\\007'; exec sleep 3600"
    set cfg to new surface configuration from {command:command_text}
    set created to new window with configuration cfg
    return id of created
  end tell
end run
APPLESCRIPT
)"
for _ in $(seq 1 24); do
  window_id="$(new_window_ids | head -1)"
  [[ "$window_id" =~ ^[1-9][0-9]*$ ]] && break
  sleep 0.25
done
if [[ ! "$window_id" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: disposable Ghostty window was not discovered" >&2
  exit 1
fi
test_pid="$(aerospace list-windows --all --format '%{window-id}|%{app-pid}' \
  | awk -F '|' -v window_id="$window_id" '$1 == window_id { print $2 }')"
if [[ ! "$test_pid" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: disposable Ghostty process was not discovered" >&2
  exit 1
fi
for _ in $(seq 1 24); do
  current_title="$(aerospace list-windows --all --format '%{window-id}|%{window-title}' \
    | awk -F '|' -v window_id="$window_id" '$1 == window_id { print $2 }')"
  if [[ "$current_title" == "$test_title" ]]; then
    test_ax_identifier="$(osascript - "$test_pid" "$test_title" 2>/dev/null <<'APPLESCRIPT' || true
on run arguments
  set target_pid to (item 1 of arguments) as integer
  set target_title to item 2 of arguments
  tell application "System Events" to tell first application process whose unix id is target_pid
    set target_window to first window whose name is target_title
    return value of attribute "AXIdentifier" of target_window
  end tell
end run
APPLESCRIPT
    )"
  fi
  [[ -n "$test_ax_identifier" ]] && break
  sleep 0.25
done
if [[ -z "$test_ax_identifier" ]]; then
  echo "FAIL: disposable Ghostty AX identifier was not discovered" >&2
  exit 1
fi
aerospace move-node-to-workspace --window-id "$window_id" fractile-workspace-test >/dev/null
aerospace workspace fractile-workspace-test >/dev/null
aerospace enable off >/dev/null

FRACTILE_CONFIG="$test_config" FRACTILE_APPLICATION_SUPPORT="$test_state" FRACTILE_MANAGED_DISPLAY=secondary "$root/target/Fractile.app/Contents/MacOS/fractile-agent" &
agent_pid=$!
sleep 2

window_minimized() {
  osascript - "$test_pid" "$test_ax_identifier" 2>/dev/null <<'APPLESCRIPT' || printf 'gone\n'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  set target_identifier to item 2 of arguments
  tell application "System Events" to tell first application process whose unix id is target_pid
    repeat with target_window in windows
      if value of attribute "AXIdentifier" of target_window is target_identifier then
        return value of attribute "AXMinimized" of target_window
      end if
    end repeat
    error "window is gone"
  end tell
end run
APPLESCRIPT
}

workspace_status() {
  osascript - "$agent_pid" <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events" to tell first application process whose unix id is target_pid
    click UI element 1 of UI element 1
    delay 0.2
    set status_text to name of menu item 1 of menu 1 of UI element 1 of UI element 1
    key code 53
    return status_text
  end tell
end run
APPLESCRIPT
}

window_position() {
  osascript - "$test_pid" "$test_ax_identifier" 2>/dev/null <<'APPLESCRIPT' || printf 'gone\n'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  set target_identifier to item 2 of arguments
  tell application "System Events" to tell first application process whose unix id is target_pid
    repeat with target_window in windows
      if value of attribute "AXIdentifier" of target_window is target_identifier then
        return position of target_window
      end if
    end repeat
    error "window is gone"
  end tell
end run
APPLESCRIPT
}

wait_for_position() {
  local comparison="$1"
  local expected="$2"
  for _ in $(seq 1 30); do
    current="$(window_position)"
    if [[ "$comparison" == "equal" && "$current" == "$expected" ]]; then
      return 0
    fi
    if [[ "$comparison" == "different" && "$current" != "$expected" && "$current" != "gone" ]]; then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

assert_not_minimized() {
  local step="$1"
  [[ "$(window_minimized)" == "false" ]] || {
    echo "FAIL: $step changed AXMinimized" >&2
    exit 1
  }
}

wait_for_workspace() {
  local expected="$1"
  for _ in $(seq 1 30); do
    [[ "$(workspace_status)" == "Workspace $expected"* ]] && return 0
    sleep 0.2
  done
  return 1
}

if [[ "$(workspace_status)" != Workspace\ 6* ]]; then
  echo "FAIL: secondary-only runtime did not start on workspace 6" >&2
  exit 1
fi
sleep 0.5
normal_position="$(window_position)"
[[ "$normal_position" != "gone" ]] || { echo "FAIL: disposable window position is unavailable" >&2; exit 1; }
assert_not_minimized "startup"

# Physical number-row keys: 7, 6, then Shift+7 to move without switching.
osascript -e 'tell application "System Events" to key code 26 using option down'
wait_for_position different "$normal_position" || { echo "FAIL: Option+7 did not move workspace 6 offscreen (status=$(workspace_status))" >&2; exit 1; }
assert_not_minimized "Option+7"
wait_for_workspace 7 || { echo "FAIL: Option+7 did not select workspace 7" >&2; exit 1; }

osascript -e 'tell application "System Events" to key code 22 using option down'
wait_for_position equal "$normal_position" || { echo "FAIL: Option+6 did not restore workspace 6 (status=$(workspace_status))" >&2; exit 1; }
assert_not_minimized "Option+6"
wait_for_workspace 6 || { echo "FAIL: Option+6 did not select workspace 6" >&2; exit 1; }

osascript - "$test_script_id" <<'APPLESCRIPT' >/dev/null
on run arguments
  set target_id to item 1 of arguments
  tell application "Ghostty" to activate window (first window whose id is target_id)
end run
APPLESCRIPT
osascript -e 'tell application "System Events" to key code 26 using {option down, shift down}'
wait_for_position different "$normal_position" || true
move_minimized="$(window_minimized)"
move_status="$(workspace_status)"
[[ "$(window_position)" != "$normal_position" ]] || { echo "FAIL: Option+Shift+7 did not move the focused window offscreen (status=$move_status)" >&2; exit 1; }
[[ "$move_minimized" == "false" ]] || { echo "FAIL: Option+Shift+7 changed AXMinimized" >&2; exit 1; }
[[ "$move_status" == Workspace\ 6* ]] || { echo "FAIL: moving a window also switched workspaces (status=$move_status)" >&2; exit 1; }

osascript -e 'tell application "System Events" to key code 26 using option down'
wait_for_position equal "$normal_position" || { echo "FAIL: moved window was not restored on workspace 7" >&2; exit 1; }
assert_not_minimized "restoring moved window"

# While the first switch is in flight, the latest hotkey must be coalesced rather than dropped.
osascript -e 'tell application "System Events" to key code 22 using option down'
osascript -e 'tell application "System Events" to key code 28 using option down'
wait_for_workspace 8 || { echo "FAIL: latest rapid workspace hotkey was dropped" >&2; exit 1; }
wait_for_position different "$normal_position" || { echo "FAIL: rapid workspace switching did not keep workspace 7 offscreen" >&2; exit 1; }
assert_not_minimized "rapid workspace switching"
osascript -e 'tell application "System Events" to key code 26 using option down'
wait_for_workspace 7 || { echo "FAIL: workspace 7 did not reactivate after rapid switching" >&2; exit 1; }
wait_for_position equal "$normal_position" || { echo "FAIL: workspace 7 did not restore after rapid switching" >&2; exit 1; }
assert_not_minimized "rapid workspace restoration"

# A hard kill must leave enough durable state for the next launch to recover the window.
osascript -e 'tell application "System Events" to key code 22 using option down'
wait_for_position different "$normal_position" || { echo "FAIL: crash test could not move workspace 7 offscreen" >&2; exit 1; }
assert_not_minimized "crash-test parking"
kill -KILL "$agent_pid"
wait "$agent_pid" 2>/dev/null || true
agent_pid=""
[[ "$(window_position)" != "$normal_position" ]] || { echo "FAIL: hard kill unexpectedly restored the parked window" >&2; exit 1; }
assert_not_minimized "hard kill"
state_file="$test_state/parked-windows.json"
[[ -s "$state_file" && "$(jq length "$state_file")" -eq 1 ]] || { echo "FAIL: parked window was not persisted before hard kill" >&2; exit 1; }

FRACTILE_CONFIG="$test_config" FRACTILE_APPLICATION_SUPPORT="$test_state" FRACTILE_MANAGED_DISPLAY=secondary "$root/target/Fractile.app/Contents/MacOS/fractile-agent" &
agent_pid=$!
sleep 2
FRACTILE_APPLICATION_SUPPORT="$test_state" "$root/target/Fractile.app/Contents/MacOS/fractile" ctl pause >/dev/null
wait_for_position equal "$normal_position" || { echo "FAIL: startup recovery did not restore the parked window after pause" >&2; exit 1; }
assert_not_minimized "crash recovery"
for _ in $(seq 1 30); do
  [[ -s "$state_file" && "$(jq length "$state_file")" -eq 0 ]] && break
  sleep 0.2
done
[[ "$(jq length "$state_file")" -eq 0 ]] || { echo "FAIL: recovered window remained in persisted state" >&2; exit 1; }

jq -n --argjson monitor "$test_monitor" --arg window "$window_id" \
  '{status: "pass", managed_display: "secondary", monitor: $monitor, disposable_window: $window, hiding: "offscreen_position", ax_minimized_unchanged: true, switch: "6 -> 7 -> 6", move_without_switch: true, rapid_hotkey_coalescing: true, crash_recovery: true}'
