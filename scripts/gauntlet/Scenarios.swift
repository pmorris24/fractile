// The scenarios. Each step drives Fractile the way a user would and asserts on both the window
// server and Fractile's own state.
import Foundation

extension Gauntlet {
    var otherWorkspace: String { homeWorkspace == "2" ? "3" : "2" }

    /// Tiles left to right, top to bottom, as the window server places them.
    func ordered() -> [TestWindow] {
        let natives = Native.windows()
        return windows.sorted { left, right in
            let a = natives[left.id]?.bounds ?? Rect(x: 0, y: 0, width: 0, height: 0)
            let b = natives[right.id]?.bounds ?? Rect(x: 0, y: 0, width: 0, height: 0)
            return a.x != b.x ? a.x < b.x : a.y < b.y
        }
    }

    func waitForAutomatic() -> (ms: Double, problem: String?) {
        wait(20, interval: 0.1) {
            guard let status = agent.status() else { return "the control socket does not answer" }
            let mode = status["mode"] as? String ?? "?"
            if mode == "automatic" { return nil }
            let error = status["last_error"] as? String ?? "none"
            return "mode is \(mode) (last error: \(error)). If it stays paused, grant Accessibility to \(options.app.path) once in System Settings › Privacy & Security › Accessibility"
        }
    }

    /// Moves focus with `focus left`/`focus right` until `target` is focused everywhere.
    func focusTile(_ target: TestWindow) throws {
        if focusProblem(target) == nil { return }
        let order = ordered()
        guard let goal = order.firstIndex(where: { $0.id == target.id }) else { return }
        for _ in 0..<order.count + 1 {
            let focused = agent.debug()?.focused
            let current = order.firstIndex { $0.id == focused } ?? order.count
            try ctl("focus", current > goal ? "left" : "right")
            _ = wait(1.5) { agent.debug()?.focused != focused ? nil : "unchanged" }
            if agent.debug()?.focused == target.id, wait(1, { focusProblem(target) }).problem == nil { return }
        }
        try require(focusProblem(target), expected: "\(target.label) focused", measured: frames(windows))
    }

    func spawn(_ label: String) throws -> TestWindow {
        switch TestWindow.spawn(label: label, bundle: bundle, directory: windowDirectory, title: "\(titleToken)-\(label)") {
        case .success(let window):
            windows.append(window)
            return window
        case .failure(let error):
            throw StepFailure(reason: "\(error)", outcome: Outcome(expected: "window \(label) opens", measured: "\(error)"))
        }
    }

    // MARK: 1. Boot and tile

    func boot() throws {
        scenario = "boot"
        try step("Fractile starts in automatic mode with isolated state") {
            if let error = agent.launch() {
                throw StepFailure(reason: error, outcome: Outcome(expected: "launch", measured: error))
            }
            let started = waitForAutomatic()
            try require(started.problem, expected: "mode automatic", measured: "\(agent.status() ?? [:])")
            let main = Native.displays().first ?? display
            guard let debug = agent.debug(),
                let home = debug.displays.first(where: { main.contains($0.frame, tolerance: 1) }) ?? debug.displays.first
            else {
                throw StepFailure(reason: "ctl debug lists no display", outcome: Outcome(expected: "a display", measured: "none"))
            }
            homeWorkspace = home.workspace
            display = home.frame
            return Outcome(
                expected: "mode automatic",
                measured: "automatic after \(Int(started.ms)) ms; workspace \(homeWorkspace) on \(display)")
        }
        try step("A and B open and tile the whole display") {
            _ = try spawn("A")
            _ = try spawn("B")
            let tiled = wait { tilingProblem(windows) }
            try require(tiled.problem, expected: "2 tiles covering \(display)", measured: frames(windows))
            return Outcome(expected: "2 tiles covering \(display)", measured: "\(Int(tiled.ms)) ms: \(frames(windows))")
        }
        try step("C opens and three tiles cover the display") {
            _ = try spawn("C")
            let tiled = wait { tilingProblem(windows) }
            try require(tiled.problem, expected: "3 tiles covering \(display)", measured: frames(windows))
            return Outcome(expected: "3 tiles covering \(display)", measured: "\(Int(tiled.ms)) ms: \(frames(windows))")
        }
    }

    // MARK: 2. Workspace switch and parking

    func switching() throws {
        scenario = "switch"
        let natives = Native.windows()
        var before: [UInt32: Rect] = [:]
        for window in windows { before[window.id] = natives[window.id]?.bounds }
        let other = otherWorkspace
        try step("switch \(other) parks every window (≤ 1 pt strip on screen)") {
            try ctl("switch", other)
            let parked = wait {
                if let problem = windows.lazy.compactMap(parkingProblem).first { return problem }
                return agent.debug()?.displays.contains { $0.workspace == other } == true ? nil : "workspace \(other) is not shown"
            }
            try require(parked.problem, expected: "every window parked", measured: frames(windows))
            return Outcome(expected: "≤ 1 pt of each window on any display", measured: "\(Int(parked.ms)) ms: \(frames(windows))")
        }
        try step("parked-windows.json holds each window's original frame") {
            var problem: String?
            let written = wait(2) {
                guard case .success(let parked) = agent.parked() else { return "unreadable" }
                for window in windows {
                    guard let frame = parked[window.id] else { return "\(window.label) is missing" }
                    guard let original = before[window.id], frame.close(to: original, tolerance: options.tolerance) else {
                        return "\(window.label) recorded at \(frame), was \(before[window.id].map { "\($0)" } ?? "?")"
                    }
                }
                return parked.count == windows.count ? nil : "\(parked.count) entries, expected \(windows.count)"
            }
            problem = written.problem
            let parked = try parkedFrames()
            let listed = windows.map { "\($0.label)=\(parked[$0.id].map { "\($0)" } ?? "missing")" }.joined(separator: " ")
            try require(problem, expected: "3 entries matching the frames before the switch", measured: listed)
            return Outcome(expected: "3 entries matching the frames before the switch", measured: listed)
        }
        try step("switch \(homeWorkspace) restores every exact frame") {
            try ctl("switch", homeWorkspace)
            let restored = wait {
                let natives = Native.windows()
                for window in windows {
                    guard let now = natives[window.id]?.bounds, let then = before[window.id] else { return "\(window.label) is gone" }
                    if !now.close(to: then, tolerance: options.tolerance) { return "\(window.label) is at \(now), was \(then)" }
                }
                return tilingProblem(windows)
            }
            try require(restored.problem, expected: "frames from before the switch", measured: frames(windows))
            return Outcome(expected: "frames from before the switch", measured: "\(Int(restored.ms)) ms: \(frames(windows))")
        }
        try step("parked-windows.json is empty again") {
            let emptied = wait(3) {
                guard case .success(let parked) = agent.parked() else { return "unreadable" }
                return parked.isEmpty ? nil : "\(parked.count) entries remain"
            }
            try require(emptied.problem, expected: "0 entries", measured: "\((try? parkedFrames().count) ?? -1) entries")
            return Outcome(expected: "0 entries", measured: "0 entries after \(Int(emptied.ms)) ms")
        }
    }

    // MARK: 3. Focus

    func focus() throws {
        scenario = "focus"
        let order = ordered()
        try step("focus left reaches the leftmost tile") {
            try focusTile(order[0])
            return Outcome(expected: "\(order[0].label) focused, frontmost and in front", measured: "\(order[0].label) focused")
        }
        try step("focus right walks every tile with real macOS focus") {
            var times: [String] = []
            for window in order.dropFirst() {
                try ctl("focus", "right")
                let moved = wait { focusProblem(window) }
                try require(moved.problem, expected: "\(window.label) focused", measured: times.joined(separator: " "))
                times.append("\(window.label) \(Int(moved.ms)) ms")
            }
            return Outcome(expected: order.dropFirst().map(\.label).joined(separator: " → "), measured: times.joined(separator: ", "))
        }
        try step("focus left steps back one tile") {
            let target = order[order.count - 2]
            try ctl("focus", "left")
            let moved = wait { focusProblem(target) }
            try require(moved.problem, expected: "\(target.label) focused", measured: label(agent.debug()?.focused))
            return Outcome(expected: "\(target.label) focused", measured: "\(target.label) in \(Int(moved.ms)) ms")
        }
        try step("repeat switch \(homeWorkspace) cycles focus within the workspace") {
            let from = agent.debug()?.focused
            try ctl("switch", homeWorkspace)
            var landed: TestWindow?
            let moved = wait {
                guard let now = agent.debug()?.focused, now != from, let window = windows.first(where: { $0.id == now }) else {
                    return "focus stayed on \(label(from))"
                }
                landed = window
                return focusProblem(window)
            }
            try require(moved.problem, expected: "focus moves off \(label(from))", measured: label(agent.debug()?.focused))
            return Outcome(expected: "focus moves off \(label(from))", measured: "\(label(from)) → \(landed?.label ?? "?") in \(Int(moved.ms)) ms")
        }
        try step("focus next moves real focus to another tile") {
            let from = agent.debug()?.focused
            try ctl("focus", "next")
            var landed: TestWindow?
            let moved = wait {
                guard let now = agent.debug()?.focused, now != from, let window = windows.first(where: { $0.id == now }) else {
                    return "focus stayed on \(label(from))"
                }
                landed = window
                return focusProblem(window)
            }
            try require(moved.problem, expected: "focus moves off \(label(from))", measured: label(agent.debug()?.focused))
            return Outcome(expected: "focus moves off \(label(from))", measured: "\(label(from)) → \(landed?.label ?? "?") in \(Int(moved.ms)) ms")
        }
    }

    // MARK: 4. Minimize and hide reflow

    func reflow() throws {
        scenario = "reflow"
        let order = ordered()
        let budget = options.reflowBudgetMs
        for (verb, undo, target) in [("minimize", "deminimize", order[2]), ("hide", "unhide", order[1])] {
            let rest = windows.filter { $0.id != target.id }
            try step("\(verb) \(target.label): the rest re-fill ≤ \(Int(budget)) ms after it leaves") {
                // Window-server polling only, every 5 ms: when macOS took the window off screen and when
                // the remaining tiles covered the display.
                var leftScreen: Double?
                var refilledAt: Double?
                let start = DispatchTime.now()
                target.send(verb)
                let settled = wait(5, interval: 0.005) {
                    let ms = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000
                    if leftScreen == nil, target.native?.onScreen == false { leftScreen = ms }
                    if refilledAt == nil, nativeCoverageProblem(rest) == nil { refilledAt = ms }
                    return leftScreen != nil && refilledAt != nil ? nil : "\(target.label) on screen or the rest not re-filled"
                }
                try require(settled.problem, expected: "\(target.label) off screen, the rest cover the display", measured: frames(windows))
                try require(wait { tilingProblem(rest) }.problem, expected: "2 tiles", measured: frames(windows))
                // The budget is Fractile's share: macOS's own minimize animation runs first, and its
                // length depends on the machine and the Dock's effect setting.
                let refill = refilledAt!
                let fractile = max(0, refill - leftScreen!)
                let measured = "Fractile re-filled \(Int(fractile)) ms after macOS took the window off screen (\(Int(leftScreen!)) ms after the command; \(Int(refill)) ms total): \(frames(rest))"
                return Outcome(
                    expected: "≤ \(Int(budget)) ms after the window leaves the screen", measured: measured,
                    overBudget: fractile > budget ? "re-fill took \(Int(fractile)) ms, budget \(Int(budget)) ms" : nil)
            }
            try step("\(undo) \(target.label): three tiles again") {
                target.send(undo)
                let back = wait { tilingProblem(windows) }
                try require(back.problem, expected: "3 tiles", measured: frames(windows))
                return Outcome(expected: "3 tiles covering the display", measured: "\(Int(back.ms)) ms: \(frames(windows))")
            }
        }
    }

    // MARK: 5. Float and scratchpad

    func float() throws {
        scenario = "float"
        let target = ordered()[0]
        let rest = windows.filter { $0.id != target.id }
        try focusTile(target)
        try step("float \(target.label): it leaves the tiles and the rest re-fill") {
            try ctl("float")
            let floated = wait {
                agent.debug()?.floating.contains(target.id) == true ? tilingProblem(rest) : "\(target.label) is not floating"
            }
            try require(floated.problem, expected: "\(target.label) floating, 2 tiles", measured: frames(windows))
            return Outcome(expected: "\(target.label) floating, 2 tiles", measured: "\(Int(floated.ms)) ms: \(frames(rest))")
        }
        try step("float \(target.label) again: three tiles") {
            try require(wait(2) { focusProblem(target) }.problem, expected: "\(target.label) still focused", measured: label(agent.debug()?.focused))
            try ctl("float")
            let tiled = wait {
                agent.debug()?.floating.isEmpty == true ? tilingProblem(windows) : "\(target.label) still floats"
            }
            try require(tiled.problem, expected: "3 tiles, nothing floating", measured: frames(windows))
            return Outcome(expected: "3 tiles, nothing floating", measured: "\(Int(tiled.ms)) ms: \(frames(windows))")
        }
    }

    func scratchpad() throws {
        scenario = "scratchpad"
        let target = ordered()[1]
        let rest = windows.filter { $0.id != target.id }
        try focusTile(target)
        try step("scratchpad mark hides \(target.label) off-screen and records it") {
            try ctl("scratchpad", "mark")
            let hidden = wait {
                if let problem = parkingProblem(target) ?? tilingProblem(rest) { return problem }
                guard case .success(let parked) = agent.parked(), parked[target.id] != nil else {
                    return "\(target.label) is not in parked-windows.json"
                }
                return nil
            }
            try require(hidden.problem, expected: "\(target.label) parked, 2 tiles", measured: frames(windows))
            return Outcome(expected: "\(target.label) parked and recorded, 2 tiles", measured: "\(Int(hidden.ms)) ms: \(frames(windows))")
        }
        try step("scratchpad toggle summons \(target.label) floating and focused") {
            try ctl("scratchpad", "toggle")
            let shown = wait {
                guard let bounds = target.native?.bounds, display.contains(bounds, tolerance: options.tolerance) else {
                    return "\(target.label) is not fully on the display"
                }
                if agent.debug()?.floating.contains(target.id) != true { return "\(target.label) is not floating" }
                return focusProblem(target) ?? tilingProblem(rest)
            }
            try require(shown.problem, expected: "\(target.label) on screen, floating, focused", measured: frames(windows))
            return Outcome(expected: "\(target.label) on screen, floating, focused", measured: "\(Int(shown.ms)) ms: \(frames(windows))")
        }
        try step("scratchpad mark on the shown window tiles it again") {
            try ctl("scratchpad", "mark")
            let tiled = wait {
                guard case .success(let parked) = agent.parked(), parked.isEmpty else { return "parked-windows.json is not empty" }
                return agent.debug()?.floating.isEmpty == true ? tilingProblem(windows) : "\(target.label) still floats"
            }
            try require(tiled.problem, expected: "3 tiles, nothing parked", measured: frames(windows))
            return Outcome(expected: "3 tiles, nothing parked", measured: "\(Int(tiled.ms)) ms: \(frames(windows))")
        }
    }

    // MARK: 6. Crash and relaunch

    /// Sends the rightmost window to the other workspace; returns it and its frame before parking.
    func parkOne() throws -> (TestWindow, Rect) {
        let target = ordered().last!
        try focusTile(target)
        try require(wait { tilingProblem(windows) }.problem, expected: "3 settled tiles", measured: frames(windows))
        let original = target.native?.bounds ?? display
        moves += 1
        try ctl("move", otherWorkspace)
        let rest = windows.filter { $0.id != target.id }
        let parked = wait {
            if let problem = parkingProblem(target) ?? tilingProblem(rest) { return problem }
            guard case .success(let parked) = agent.parked(), let frame = parked[target.id] else {
                return "\(target.label) is not in parked-windows.json"
            }
            return frame.close(to: original, tolerance: options.tolerance) ? nil : "\(target.label) recorded at \(frame), was \(original)"
        }
        try require(parked.problem, expected: "\(target.label) parked at its recorded frame", measured: frames(windows))
        return (target, original)
    }

    func crash() throws {
        scenario = "crash"
        var target: TestWindow!
        var original = display
        try step("move a window to workspace \(otherWorkspace): it parks with its frame recorded and hands focus to a remaining tile") {
            (target, original) = try parkOne()
            var landed: TestWindow?
            let focused = wait {
                guard let now = agent.debug()?.focused, now != target.id,
                    let window = windows.first(where: { $0.id == now })
                else { return "Fractile reports \(label(agent.debug()?.focused)) focused" }
                landed = window
                return focusProblem(window)
            }
            try require(focused.problem, expected: "a remaining visible tile focused", measured: label(agent.debug()?.focused))
            return Outcome(
                expected: "\(target.label) parked at \(original), focus handed to a remaining tile",
                measured: "\(frames(windows)); focus -> \(landed?.label ?? "?") in \(Int(focused.ms)) ms")
        }
        try step("SIGKILL leaves \(target.label) parked and its record on disk") {
            agent.hardKill()
            let dead = wait(5) { agent.pid == nil ? nil : "agent still running" }
            try require(dead.problem, expected: "agent gone", measured: "pid \(agent.pid ?? 0)")
            let parked = try parkedFrames()
            try require(
                parkingProblem(target) ?? (parked[target.id]?.close(to: original, tolerance: options.tolerance) == true ? nil : "record lost"),
                expected: "\(target.label) off-screen, recorded at \(original)",
                measured: "\(frames([target])), record \(parked[target.id].map { "\($0)" } ?? "missing")")
            return Outcome(expected: "\(target.label) off-screen, recorded at \(original)", measured: "record \(parked[target.id]!)")
        }
        try step("relaunch keeps \(target.label) on workspace \(otherWorkspace)") {
            if let error = agent.launch() {
                throw StepFailure(reason: error, outcome: Outcome(expected: "launch", measured: error))
            }
            try require(waitForAutomatic().problem, expected: "mode automatic", measured: "\(agent.status() ?? [:])")
            let rest = windows.filter { $0.id != target.id }
            let kept = wait { parkingProblem(target) ?? tilingProblem(rest) }
            try require(kept.problem, expected: "\(target.label) parked, 2 tiles", measured: frames(windows))
            try ctl("switch", otherWorkspace)
            let shown = wait { tilingProblem([target], workspace: otherWorkspace) }
            try require(shown.problem, expected: "\(target.label) fills workspace \(otherWorkspace)", measured: frames(windows))
            try ctl("switch", homeWorkspace)
            let back = wait { parkingProblem(target) ?? tilingProblem(rest) }
            try require(back.problem, expected: "\(target.label) parked again", measured: frames(windows))
            return Outcome(expected: "\(target.label) on \(otherWorkspace), others tiled", measured: frames(windows))
        }
    }
}
