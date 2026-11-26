// The last scenario always runs: a clean quit must put every parked window back, and nothing
// Fractile wrote may carry a panic or a window title.
import Foundation

extension Gauntlet {
    func recovery() {
        scenario = "recovery"
        var restoredFrames: [UInt32: Rect] = [:]
        if agent.pid == nil {
            skip("quit restores every parked window on screen", "Fractile is not running")
        } else {
            // An aborted scenario may leave a window minimized or hidden; bring everything back first.
            for window in windows {
                window.send("deminimize")
                window.send("unhide")
            }
            _ = wait(3) { windows.contains { $0.native?.onScreen != true } ? "still off screen" : nil }
            _ = wait { tilingProblem(windows) }
            try? step("quit restores every parked window on screen") {
                if windows.count > 1, (try parkedFrames()).isEmpty { _ = try parkOne() }
                restoredFrames = try parkedFrames()
                let parkedLabels = restoredFrames.keys.map(label).sorted().joined(separator: ",")
                try ctl("quit")
                let exited = wait(15, interval: 0.1) { agent.pid == nil ? nil : "agent still running" }
                try require(exited.problem, expected: "agent exits", measured: "pid \(agent.pid ?? 0)")
                let restored = wait {
                    let natives = Native.windows()
                    let screens = Native.displays()
                    for window in windows {
                        guard let bounds = natives[window.id]?.bounds else { return "\(window.label) is gone" }
                        if let frame = restoredFrames[window.id], !bounds.close(to: frame, tolerance: options.tolerance) {
                            return "\(window.label) is at \(bounds), its recorded frame is \(frame)"
                        }
                        let visible = screens.reduce(0) { $0 + bounds.intersection($1).area }
                        if visible < bounds.area * 0.99 { return "\(window.label) at \(bounds) is not fully on screen" }
                    }
                    return nil
                }
                try require(restored.problem, expected: "[\(parkedLabels)] back at their recorded frames", measured: frames(windows))
                return Outcome(
                    expected: "[\(parkedLabels)] back at their recorded frames, all on screen",
                    measured: "exit \(Int(exited.ms)) ms, restored \(Int(restored.ms)) ms: \(frames(windows))")
            }
        }
        // Whatever happened above, Fractile stops before its log and journal are judged.
        stopAgent()
        try? step("parked-windows.json is empty after quit") {
            let parked = try parkedFrames()
            try require(parked.isEmpty ? nil : "\(parked.count) entries remain", expected: "0 entries", measured: "\(parked.count) entries")
            return Outcome(expected: "0 entries", measured: "0 entries")
        }
        try? step("agent.log has no panics or errors") {
            let log = (try? String(contentsOf: agent.state.appendingPathComponent("agent.log"), encoding: .utf8)) ?? ""
            let lines = log.split(separator: "\n").map(String.init)
            let bad = lines.filter { line in
                let lower = line.lowercased()
                return lower.contains("panic") || lower.contains("error") || lower.contains("fatal")
            }
            try require(
                bad.isEmpty ? nil : "\(bad.count) bad lines, first: \(bad[0])",
                expected: "0 panic/error/fatal lines", measured: bad.prefix(5).joined(separator: " | "))
            return Outcome(expected: "0 panic/error/fatal lines", measured: "0 of \(lines.count) lines")
        }
        let doctor = agent.doctor()
        try? doctor.out.write(to: options.out.appendingPathComponent("doctor.txt"), atomically: true, encoding: .utf8)
        try? step("no window title reaches logs, snapshots, doctor, or non-memory state files") {
            var scanned = 0
            var leaks: [String] = []
            let roots = [agent.state, options.out.appendingPathComponent("snapshots")]
            for root in roots {
                let files = FileManager.default.enumerator(at: root, includingPropertiesForKeys: nil)
                while let url = files?.nextObject() as? URL {
                    if url.lastPathComponent == "window-memory.json", root == agent.state {
                        if let attrs = try? FileManager.default.attributesOfItem(atPath: url.path),
                            let posix = (attrs[.posixPermissions] as? NSNumber)?.intValue,
                            posix & 0o777 != 0o600
                        {
                            leaks.append("window-memory.json mode \(String(posix & 0o777, radix: 8))")
                        }
                        continue
                    }
                    guard let data = try? Data(contentsOf: url) else { continue }
                    scanned += 1
                    if data.range(of: Data(titleToken.utf8)) != nil { leaks.append(url.lastPathComponent) }
                }
            }
            if doctor.out.contains(titleToken) { leaks.append("fractile doctor") }
            try require(leaks.isEmpty ? nil : "violation in \(leaks.joined(separator: ", "))", expected: "0 leaks (window-memory.json 0600)", measured: leaks.joined(separator: ", "))
            return Outcome(expected: "title token in 0 non-memory files", measured: "0 of \(scanned) files and doctor output")
        }
        try? step("fractile doctor reports a healthy run") {
            try require(doctor.status == 0 ? nil : "doctor exited \(doctor.status)", expected: "exit 0", measured: doctor.out)
            let expectedStops = agent.launches - agent.kills
            var problems: [String] = []
            if let match = capture(#"Agent starts: (\d+), clean stops: (\d+)"#, in: doctor.out), match.count == 2 {
                if match != ["\(agent.launches)", "\(expectedStops)"] {
                    problems.append("starts \(match[0]) / clean stops \(match[1]), expected \(agent.launches) / \(expectedStops)")
                }
            } else {
                problems.append("no start/stop line")
            }
            // A deliberate `ctl move` soon after placement is journaled as a wrong placement.
            let wrong = capture(#"Wrong placements[^:]*: (\d+)"#, in: doctor.out).flatMap { Int($0.first ?? "") } ?? -1
            if wrong < 0 || wrong > moves { problems.append("\(wrong) wrong placements, at most \(moves) expected") }
            if !doctor.out.contains("Anomalies (lasting at least 3s):\n  none") { problems.append("anomalies recorded") }
            let summary = "starts \(agent.launches), clean stops \(expectedStops), ≤ \(moves) wrong placements, no anomalies"
            try require(problems.isEmpty ? nil : problems.joined(separator: "; "), expected: summary, measured: "see doctor.txt")
            return Outcome(expected: summary, measured: summary)
        }
    }

    /// Quits Fractile the clean way (which restores parked windows), then with SIGTERM if it hangs.
    func stopAgent() {
        guard agent.pid != nil else { return }
        _ = agent.ctl(["quit"])
        _ = wait(10, interval: 0.1) { agent.pid == nil ? nil : "running" }
        if let pid = agent.pid { kill(pid, SIGTERM) }
        _ = wait(5, interval: 0.1) { agent.pid == nil ? nil : "running" }
    }

    /// Closes the test windows and makes sure no Fractile under test outlives the run, restoring first.
    func cleanup() {
        stopAgent()
        for window in windows { window.send("quit") }
        _ = wait(3, interval: 0.1) { windows.contains { processAlive($0.pid) } ? "open" : nil }
        for window in windows where processAlive(window.pid) { kill(window.pid, SIGKILL) }
    }
}

private func capture(_ pattern: String, in text: String) -> [String]? {
    guard let regex = try? NSRegularExpression(pattern: pattern),
        let match = regex.firstMatch(in: text, range: NSRange(text.startIndex..., in: text))
    else { return nil }
    return (1..<match.numberOfRanges).compactMap { index in
        Range(match.range(at: index), in: text).map { String(text[$0]) }
    }
}
