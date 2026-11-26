#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
output="${TMPDIR:-/tmp}/fractile-ghostty-live.json"
initial_output="${output%.json}-initial.json"
baseline_pids="$(mktemp)"
baseline_workspaces="$(mktemp)"
watch_pid=""
test_pid=""

cd "$root"
if ! aerospace list-monitors >/dev/null 2>&1; then
  echo "FAIL: AeroSpace must be enabled before running this harness" >&2
  rm -f "$baseline_pids" "$baseline_workspaces"
  exit 1
fi
previous_workspace="$(aerospace list-workspaces --focused)"
previous_monitor="$(aerospace list-monitors --focused --format '%{monitor-id}')"
test_monitor="$(aerospace list-monitors --format '%{monitor-id}' | grep -v "^${previous_monitor}$" | head -1)"
if [[ ! "$test_monitor" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: a second AeroSpace monitor is required" >&2
  rm -f "$baseline_pids" "$baseline_workspaces"
  exit 1
fi
previous_bundle="$(osascript -e 'tell application "System Events" to get bundle identifier of first application process whose frontmost is true')"
pgrep -f '^/Applications/Ghostty.app/Contents/MacOS/ghostty' | sort >"$baseline_pids" || true
aerospace list-windows --all --format '%{window-id}|%{workspace}' | sort -t '|' -k1,1 >"$baseline_workspaces"

new_ghostty_pids() {
  comm -13 "$baseline_pids" <(
    pgrep -f '^/Applications/Ghostty.app/Contents/MacOS/ghostty' | sort || true
  )
}

test_window_ids() {
  [[ -n "$test_pid" ]] || return 0
  aerospace list-windows --all --format '%{window-id}|%{app-pid}' \
    | awk -F '|' -v test_pid="$test_pid" '$2 == test_pid { print $1 }'
}

restore_baseline_workspaces() {
  local current_workspaces
  current_workspaces="$(mktemp)"
  aerospace list-windows --all --format '%{window-id}|%{workspace}' \
    | sort -t '|' -k1,1 >"$current_workspaces"
  join -t '|' -1 1 -2 1 "$baseline_workspaces" "$current_workspaces" \
    | while IFS='|' read -r window_id baseline_workspace current_workspace; do
        if [[ "$baseline_workspace" != "$current_workspace" ]]; then
          aerospace move-node-to-workspace --window-id "$window_id" "$baseline_workspace" \
            >/dev/null 2>&1
        fi
      done
  rm -f "$current_workspaces"
}

baseline_workspaces_restored() {
  local current_workspaces
  current_workspaces="$(mktemp)"
  aerospace list-windows --all --format '%{window-id}|%{workspace}' \
    | sort -t '|' -k1,1 >"$current_workspaces"
  if ! join -t '|' -1 1 -2 1 "$baseline_workspaces" "$current_workspaces" \
    | awk -F '|' '$2 != $3 { changed = 1 } END { exit changed }'; then
    rm -f "$current_workspaces"
    return 1
  fi
  rm -f "$current_workspaces"
  return 0
}

restore_environment() {
  set +e
  if [[ -n "$watch_pid" ]]; then
    kill "$watch_pid" 2>/dev/null
  fi
  aerospace enable on >/dev/null 2>&1
  repeat=0
  while (( repeat < 4 )); do
    sleep 0.5
    while read -r window_id; do
      [[ -n "$window_id" ]] && aerospace close --window-id "$window_id" 2>/dev/null
    done < <(test_window_ids)
    repeat=$((repeat + 1))
  done
  if [[ -n "$test_pid" ]] && kill -0 "$test_pid" 2>/dev/null; then
    kill "$test_pid" 2>/dev/null
  fi
  restore_baseline_workspaces
  aerospace workspace "$previous_workspace" >/dev/null 2>&1
  osascript - "$previous_bundle" <<'APPLESCRIPT' 2>/dev/null
on run arguments
  tell application id (item 1 of arguments) to activate
end run
APPLESCRIPT
  rm -f "$baseline_pids" "$baseline_workspaces"
}
trap restore_environment EXIT

aerospace workspace fractile-test >/dev/null 2>&1
aerospace move-workspace-to-monitor --workspace fractile-test "$test_monitor" 2>/dev/null
aerospace workspace fractile-test
open -na /Applications/Ghostty.app

attempt=0
while (( attempt < 24 )); do
  test_pid="$(new_ghostty_pids | head -1)"
  [[ "$test_pid" =~ ^[1-9][0-9]*$ ]] && break
  sleep 0.25
  attempt=$((attempt + 1))
done
if [[ ! "$test_pid" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: could not discover the disposable Ghostty process" >&2
  exit 1
fi
attempt=0
window_id=""
while (( attempt < 24 )); do
  window_id="$(test_window_ids | head -1)"
  [[ "$window_id" =~ ^[1-9][0-9]*$ ]] && break
  sleep 0.25
  attempt=$((attempt + 1))
done
if [[ ! "$window_id" =~ ^[1-9][0-9]*$ ]]; then
  echo "FAIL: could not discover the disposable Ghostty window" >&2
  exit 1
fi
aerospace move-node-to-workspace --window-id "$window_id" fractile-test 2>/dev/null
aerospace workspace fractile-test 2>/dev/null
actual_monitor="$(aerospace list-windows --all --format '%{window-id}|%{monitor-id}' \
  | awk -F '|' -v window_id="$window_id" '$1 == window_id { print $2 }')"
if [[ "$actual_monitor" != "$test_monitor" ]]; then
  echo "FAIL: disposable window landed on monitor $actual_monitor instead of $test_monitor" >&2
  exit 1
fi
aerospace enable off
sleep 1

osascript - "$test_pid" >/dev/null <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events" to tell first application process whose unix id is target_pid to set frontmost to true
end run
APPLESCRIPT
sleep 0.5

initial_window_menu_items="$(osascript - "$test_pid" <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events" to tell first application process whose unix id is target_pid to count menu items of menu 1 of menu bar item "Window" of menu bar 1
end run
APPLESCRIPT
)"
if (( initial_window_menu_items < 1 )); then
  echo "FAIL: the isolated Ghostty process had no native Window menu" >&2
  exit 1
fi

cargo run --locked --quiet -- watch-window "$window_id" 1 >"$initial_output"
if ! jq -e --argjson window_id "$window_id" '
  any(.steps[0].observation.applications[]?.windows[]?; .id.window_id == $window_id)
' "$initial_output" >/dev/null; then
  echo "FAIL: isolated Ghostty window $window_id did not become eligible" >&2
  exit 1
fi

cargo run --locked --quiet -- watch-window "$window_id" 64 --apply >"$output" &
watch_pid=$!
sleep 1

osascript - "$test_pid" >/dev/null <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events"
    tell first application process whose unix id is target_pid
      set frontmost to true
      delay 0.5
      repeat 3 times
        click menu item "New Tab" of menu 1 of menu bar item "File" of menu bar 1
        delay 0.6
      end repeat
      click menu item "Show Previous Tab" of menu 1 of menu bar item "Window" of menu bar 1
      delay 0.6
      click menu item "Show Next Tab" of menu 1 of menu bar item "Window" of menu bar 1
    end tell
  end tell
end run
APPLESCRIPT

sleep 2
tabbed_window_menu_items="$(osascript - "$test_pid" <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events" to tell first application process whose unix id is target_pid to count menu items of menu 1 of menu bar item "Window" of menu bar 1
end run
APPLESCRIPT
)"
osascript - "$test_pid" >/dev/null <<'APPLESCRIPT'
on run arguments
  set target_pid to (item 1 of arguments) as integer
  tell application "System Events"
    tell first application process whose unix id is target_pid
      click menu item "Close All Windows" of menu 1 of menu bar item "File" of menu bar 1
    end tell
  end tell
end run
APPLESCRIPT

wait "$watch_pid"
watch_pid=""
aerospace enable on >/dev/null
sleep 0.5
while read -r test_window_id; do
  [[ -n "$test_window_id" ]] && aerospace close --window-id "$test_window_id" >/dev/null 2>&1
done < <(test_window_ids)
restore_baseline_workspaces

baseline="$(jq 'first(.steps[].reconciliation.model.workspace.slots | length | select(. > 0)) // 0' "$output")"
peak="$(jq '[.steps[].reconciliation.model.workspace.slots | length] | max' "$output")"
final="$(jq '.steps[-1].reconciliation.model.workspace.slots | length' "$output")"
managed_cycles="$(jq '[.steps[] | select((.reconciliation.model.workspace.slots | length) == 1)] | length' "$output")"
replacements="$(jq '[.steps[].reconciliation.decisions[] | select(.type == "native_tab_replaced")] | length' "$output")"
ambiguities="$(jq '[.steps[].reconciliation.decisions[] | select(.type == "native_tab_ambiguous")] | length' "$output")"
slot_ids="$(jq '[.steps[].reconciliation.model.workspace.slots[].id] | unique | length' "$output")"
insertions="$(jq '[.steps[].reconciliation.decisions[] | select(.type == "window_inserted")] | length' "$output")"
removals="$(jq '[.steps[].reconciliation.decisions[] | select(.type == "window_removed")] | length' "$output")"
physical_ids="$(jq '[.steps[].observation.applications[] | select(.status == "complete") | .windows[].id.window_id] | unique | length' "$output")"
window_menu_delta=$((tabbed_window_menu_items - initial_window_menu_items))
applied="$(jq '[.steps[].apply_results[]? | select(.outcome == "applied")] | length' "$output")"
apply_failures="$(jq '[.steps[].apply_results[]? | select(.outcome == "unavailable" or .outcome == "constrained")] | length' "$output")"
pending_frames="$(jq '[
  .steps[] as $step
  | $step.reconciliation.placements[] as $placement
  | select($placement.window != null)
  | $step.observation.applications[]?
  | select(.status == "complete")
  | .windows[]?
  | select(.id == $placement.window)
  | select(
      (.frame.x - $placement.frame.x > 2) or ($placement.frame.x - .frame.x > 2)
      or (.frame.y - $placement.frame.y > 2) or ($placement.frame.y - .frame.y > 2)
      or (.frame.width - $placement.frame.width > 2) or ($placement.frame.width - .frame.width > 2)
      or (.frame.height - $placement.frame.height > 2) or ($placement.frame.height - .frame.height > 2)
    )
] | length' "$output")"

if (( baseline != 1 || peak != 1 || managed_cycles < 2 )); then
  echo "FAIL: native tabs changed the logical slot count (baseline=$baseline peak=$peak managed_cycles=$managed_cycles)" >&2
  exit 1
fi
if (( final != 0 )); then
  echo "FAIL: closing the test window did not remove its slot (final=$final)" >&2
  exit 1
fi
if (( slot_ids != 1 || insertions != 1 || removals != 1 )); then
  echo "FAIL: the logical slot churned (slot_ids=$slot_ids insertions=$insertions removals=$removals)" >&2
  exit 1
fi
if kill -0 "$test_pid" 2>/dev/null && (( $(test_window_ids | wc -l) > 0 )); then
  echo "FAIL: isolated Ghostty test windows leaked" >&2
  exit 1
fi
if (( physical_ids < 4 || replacements < 3 )); then
  echo "FAIL: native physical replacement was not exercised (physical_ids=$physical_ids replacements=$replacements)" >&2
  exit 1
fi
if (( (pending_frames > 0 && applied < 1) || apply_failures != 0 )); then
  echo "FAIL: frame application did not converge cleanly (pending=$pending_frames applied=$applied failures=$apply_failures)" >&2
  exit 1
fi
if ! baseline_workspaces_restored; then
  echo "FAIL: baseline AeroSpace workspaces were not restored" >&2
  exit 1
fi

jq -n \
  --arg output "$output" \
  --argjson baseline "$baseline" \
  --argjson peak "$peak" \
  --argjson final "$final" \
  --argjson replacements "$replacements" \
  --argjson ambiguities "$ambiguities" \
  --argjson physical_ids "$physical_ids" \
  --argjson test_monitor "$test_monitor" \
  --argjson window_menu_delta "$window_menu_delta" \
  --argjson created_native_tabs 3 \
  --argjson applied "$applied" \
  --argjson pending_frames "$pending_frames" \
  '{status: "pass", output: $output, test_monitor: $test_monitor, baseline_slots: $baseline, peak_slots: $peak, final_slots: $final, created_native_tabs: $created_native_tabs, window_menu_delta: $window_menu_delta, physical_window_ids: $physical_ids, native_tab_replacements: $replacements, native_tab_ambiguities: $ambiguities, pending_frames: $pending_frames, applied_frames: $applied}'
