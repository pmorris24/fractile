// Steps, waits, shared assertions, and the receipt.
import AppKit
import Foundation

struct Outcome {
    var expected: String
    var measured: String
    /// A failure that leaves the windows in a known state (a missed time budget): the step fails
    /// but the scenario carries on.
    var overBudget: String? = nil
}

struct StepFailure: Error {
    let reason: String
    let outcome: Outcome
}

struct StepRecord {
    let index: Int
    let scenario: String
    let name: String
    let status: String
    let ms: Double
    let outcome: Outcome
    let reason: String?

    var json: [String: Any] {
        var object: [String: Any] = [
            "index": index, "scenario": scenario, "step": name, "status": status,
            "ms": (ms * 10).rounded() / 10, "expected": outcome.expected, "measured": outcome.measured,
        ]
        if let reason { object["reason"] = reason }
        return object
    }
}

struct Options {
    var app: URL
    var out: URL
    var only: Set<String>?
    var reflowBudgetMs: Double = 500
    var settleTimeout: Double = 5
    var tolerance: Double = 2
}

final class Gauntlet {
    let options: Options
    let agent: Agent
    let bundle: URL
    let windowDirectory: URL
    /// Every test window's title starts with this. It must never appear in Fractile's output.
    let titleToken: String
    var windows: [TestWindow] = []
    var display = Rect(x: 0, y: 0, width: 0, height: 0)
    var homeWorkspace = "1"
    var scenario = "preflight"
    /// `ctl move` commands issued. The journal counts a move soon after placement as a wrong placement.
    var moves = 0
    private(set) var records: [StepRecord] = []
    private let started = Date()

    init(options: Options, bundle: URL) throws {
        self.options = options
        self.bundle = bundle
        let fileManager = FileManager.default
        let state = options.out.appendingPathComponent("state")
        windowDirectory = options.out.appendingPathComponent("windows")
        for directory in [options.out, state, windowDirectory, options.out.appendingPathComponent("snapshots")] {
            try fileManager.createDirectory(at: directory, withIntermediateDirectories: true)
        }
        try fileManager.setAttributes([.posixPermissions: 0o700], ofItemAtPath: state.path)
        // Every app already running is ignored, so Fractile tiles only the test windows and never moves,
        // parks or focuses anything else on the machine.
        let ignored = Set(NSWorkspace.shared.runningApplications.compactMap(\.bundleIdentifier))
            .subtracting(["app.fractile", "app.fractile.gauntlet"])
            .filter { !$0.contains("\"") && !$0.contains("\\") }
            .sorted()
        let rules = ignored.map { "[[rules]]\nbundle_id = \"\($0)\"\naction = \"ignore\"\n" }
        let config = options.out.appendingPathComponent("config.toml")
        try ("""
            version = 1

            [settings]
            hud = false

            [labs]
            scratchpad = true

            """ + rules.joined(separator: "\n")).write(to: config, atomically: true, encoding: .utf8)
        // A finished setup tour, so tiling starts as soon as Accessibility and hotkeys are available.
        try "".write(to: state.appendingPathComponent("onboarding-complete"), atomically: true, encoding: .utf8)
        titleToken = "gauntlet-" + String(UInt64.random(in: 0...UInt64.max), radix: 16)
        agent = Agent(app: options.app, state: state, config: config)
    }

    func selected(_ name: String) -> Bool { options.only?.contains(name) ?? true }

    // MARK: Steps

    func step(_ name: String, _ body: () throws -> Outcome) throws {
        let start = DispatchTime.now()
        do {
            let outcome = try body()
            record(name, outcome.overBudget == nil ? "pass" : "fail", start, outcome, outcome.overBudget)
        } catch let failure as StepFailure {
            record(name, "fail", start, failure.outcome, failure.reason)
            throw failure
        }
    }

    func skip(_ name: String, _ reason: String) {
        records.append(
            StepRecord(
                index: records.count + 1, scenario: scenario, name: name, status: "skip", ms: 0,
                outcome: Outcome(expected: "", measured: ""), reason: reason))
    }

    private func record(_ name: String, _ status: String, _ start: DispatchTime, _ outcome: Outcome, _ reason: String?) {
        let ms = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000
        let entry = StepRecord(
            index: records.count + 1, scenario: scenario, name: name, status: status, ms: ms,
            outcome: outcome, reason: reason)
        records.append(entry)
        snapshot(entry)
        let mark = status == "pass" ? "PASS" : "FAIL"
        print("\(mark)  \(scenario.padding(toLength: 11, withPad: " ", startingAt: 0)) \(name.padding(toLength: 62, withPad: " ", startingAt: 0)) \(String(format: "%6.0f", ms)) ms")
        if let reason { print("      \(reason)") }
        fflush(stdout)
    }

    /// Title-free evidence after every step: Fractile's view and the window server's.
    private func snapshot(_ entry: StepRecord) {
        var object: [String: Any] = ["step": entry.json]
        object["debug"] = agent.debug()?.raw ?? NSNull()
        object["status"] = agent.status() ?? NSNull()
        let natives = Native.windows()
        object["windows"] = windows.map { window -> [String: Any] in
            ["label": window.label, "native": natives[window.id]?.json ?? NSNull()]
        }
        let name = String(format: "%02d-%@.json", entry.index, entry.scenario)
        writeJSON(object, to: options.out.appendingPathComponent("snapshots").appendingPathComponent(name))
    }

    // MARK: Waiting

    /// Polls `problem` until it returns nil or `timeout` passes. Returns the elapsed milliseconds and the
    /// last problem, nil on success.
    func wait(_ timeout: Double? = nil, interval: Double = 0.02, _ problem: () -> String?) -> (ms: Double, problem: String?) {
        let start = DispatchTime.now()
        let deadline = Date(timeIntervalSinceNow: timeout ?? options.settleTimeout)
        var last: String?
        repeat {
            last = problem()
            if last == nil { break }
            Native.pump(interval)
        } while Date() < deadline
        if last != nil { last = problem() }
        return (Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1_000_000, last)
    }

    func require(_ problem: String?, expected: String, measured: @autoclosure () -> String) throws {
        if let problem {
            throw StepFailure(reason: problem, outcome: Outcome(expected: expected, measured: measured()))
        }
    }

    func ctl(_ arguments: String...) throws {
        let result = agent.ctl(arguments)
        try require(
            result.ok ? nil : "fractile ctl \(arguments.joined(separator: " ")) failed: \(result.out)",
            expected: "ctl accepted", measured: result.out)
    }

    // MARK: Shared assertions

    func label(_ id: UInt32?) -> String {
        guard let id else { return "none" }
        return windows.first { $0.id == id }?.label ?? "window \(id)"
    }

    func frames(_ windows: [TestWindow]) -> String {
        let natives = Native.windows()
        return windows.map { "\($0.label)=\(natives[$0.id].map { "\($0.bounds)" } ?? "gone")" }
            .joined(separator: " ")
    }

    /// Tiles exactly cover the display: inside it, no overlap, and no uncovered area.
    func coverageProblem(_ frames: [(String, Rect)], _ display: Rect) -> String? {
        let tolerance = options.tolerance
        for (name, frame) in frames where !display.contains(frame, tolerance: tolerance) {
            return "\(name) at \(frame) is outside the display \(display)"
        }
        for i in frames.indices {
            for j in frames.indices where j > i {
                let overlap = frames[i].1.intersection(frames[j].1)
                if overlap.width > tolerance && overlap.height > tolerance {
                    return "\(frames[i].0) and \(frames[j].0) overlap by \(overlap)"
                }
            }
        }
        let covered = frames.reduce(0) { $0 + $1.1.intersection(display).area }
        let slack = (display.width + display.height) * 2 * tolerance
        if abs(covered - display.area) > slack {
            return "tiles cover \(Int(covered)) of \(Int(display.area)) square points"
        }
        return nil
    }

    /// Window-server-only check, cheap enough for tight timing loops.
    func nativeCoverageProblem(_ windows: [TestWindow]) -> String? {
        let natives = Native.windows()
        var frames: [(String, Rect)] = []
        for window in windows {
            guard let native = natives[window.id], native.onScreen else { return "\(window.label) is not on screen" }
            frames.append((window.label, native.bounds))
        }
        return coverageProblem(frames, display)
    }

    /// Fractile's tiles on `workspace` are exactly `windows`, the window server agrees with every
    /// frame, and together they cover the display.
    func tilingProblem(_ windows: [TestWindow], workspace: String? = nil) -> String? {
        let workspace = workspace ?? homeWorkspace
        guard let debug = agent.debug() else { return "fractile ctl debug failed" }
        guard let shown = debug.displays.first(where: { $0.workspace == workspace }) else {
            return "workspace \(workspace) is not on any display"
        }
        let tiles = debug.tiles(on: workspace)
        let expected = Set(windows.map(\.id))
        if Set(tiles.keys) != expected {
            return "Fractile tiles [\(tiles.keys.map(label).sorted().joined(separator: ","))] on \(workspace), expected [\(windows.map(\.label).joined(separator: ","))]"
        }
        let natives = Native.windows()
        var frames: [(String, Rect)] = []
        for window in windows {
            guard let native = natives[window.id] else { return "\(window.label) is gone from the window server" }
            let tile = tiles[window.id]!
            if !native.bounds.close(to: tile, tolerance: options.tolerance) {
                return "\(window.label) is at \(native.bounds) but Fractile's tile is \(tile)"
            }
            frames.append((window.label, native.bounds))
        }
        return coverageProblem(frames, shown.frame)
    }

    /// Parked: at most a one-point strip of the window shows on any display.
    func parkingProblem(_ window: TestWindow) -> String? {
        guard let native = window.native else { return "\(window.label) is gone from the window server" }
        for screen in Native.displays() {
            let visible = native.bounds.intersection(screen)
            if visible.width > 1 && visible.height > 1 {
                return "\(window.label) at \(native.bounds) shows \(Int(visible.width))x\(Int(visible.height)) on display \(screen)"
            }
        }
        return nil
    }

    /// Focus agrees everywhere: Fractile's focused window, the frontmost app, and the front window on screen.
    func focusProblem(_ window: TestWindow) -> String? {
        guard let debug = agent.debug() else { return "fractile ctl debug failed" }
        if debug.focused != window.id { return "Fractile reports \(label(debug.focused)) focused" }
        let frontmost = Native.frontmostPid()
        if frontmost != window.pid {
            return "frontmost app is \(windows.first { $0.pid == frontmost }?.label ?? "pid \(frontmost ?? 0)")"
        }
        let front = Native.frontWindow(on: display)
        if front?.id != window.id { return "front window on screen is \(label(front?.id))" }
        return nil
    }

    func parkedFrames() throws -> [UInt32: Rect] {
        switch agent.parked() {
        case .success(let parked): return parked
        case .failure(let error): throw StepFailure(reason: "\(error)", outcome: Outcome(expected: "valid JSON", measured: "\(error)"))
        }
    }

    // MARK: Receipt

    var failed: Bool { records.contains { $0.status == "fail" } }

    func writeReceipt(artifacts: [String]) {
        let passed = records.filter { $0.status == "pass" }.count
        let failures = records.filter { $0.status == "fail" }.count
        let skipped = records.filter { $0.status == "skip" }.count
        let version = ProcessInfo.processInfo.operatingSystemVersion
        let receipt: [String: Any] = [
            "gauntlet": 1,
            "result": failures == 0 && passed > 0 ? "pass" : "fail",
            "started_at": ISO8601DateFormatter().string(from: started),
            "duration_ms": (Date().timeIntervalSince(started) * 1000).rounded(),
            "machine": [
                "macos": "\(version.majorVersion).\(version.minorVersion).\(version.patchVersion)",
                "displays": Native.displays().map(\.json),
            ],
            "fractile": ["version": agent.version, "app": options.app.path],
            "options": ["reflow_budget_ms": options.reflowBudgetMs, "tolerance_pt": options.tolerance],
            "counts": ["pass": passed, "fail": failures, "skip": skipped],
            "steps": records.map(\.json),
            "artifacts": artifacts,
        ]
        writeJSON(receipt, to: options.out.appendingPathComponent("receipt.json"))
        print("")
        print(String(format: "%@  %d passed, %d failed, %d skipped in %.1f s",
            failures == 0 && passed > 0 ? "PASS" : "FAIL", passed, failures, skipped,
            Date().timeIntervalSince(started)))
        print("receipt: \(options.out.appendingPathComponent("receipt.json").path)")
    }
}

func writeJSON(_ object: Any, to url: URL) {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    else { return }
    try? data.write(to: url)
}
