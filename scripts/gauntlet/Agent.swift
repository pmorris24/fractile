// The Spaceman under test: launched with its own state and config, read through `spaceman ctl`
// and its state files.
import Foundation

func workspaceName(_ value: Any?) -> String {
    if let number = value as? NSNumber { return number.stringValue }
    return value as? String ?? "?"
}

private func windowId(_ value: Any?) -> UInt32? {
    ((value as? [String: Any])?["window_id"] as? NSNumber)?.uint32Value
}

/// `spaceman ctl debug`, which is title-free by design.
struct Debug {
    let raw: [String: Any]

    var focused: UInt32? { windowId(raw["focused"]) }

    var activeWorkspaces: [String] {
        (raw["active_workspaces"] as? [Any] ?? []).map(workspaceName)
    }

    var displays: [(workspace: String, frame: Rect)] {
        (raw["displays"] as? [[String: Any]] ?? []).compactMap { display in
            Rect(json: display["frame"]).map { (workspaceName(display["workspace"]), $0) }
        }
    }

    func tiles(on workspace: String) -> [UInt32: Rect] {
        var result: [UInt32: Rect] = [:]
        for tile in raw["tiles"] as? [[String: Any]] ?? [] where workspaceName(tile["workspace"]) == workspace {
            if let id = windowId(tile["window"]), let frame = Rect(json: tile["frame"]) {
                result[id] = frame
            }
        }
        return result
    }

    var floating: [UInt32] {
        (raw["floating"] as? [[String: Any]] ?? []).compactMap { windowId($0["window"]) }
    }

    func target(_ direction: String) -> UInt32? {
        windowId((raw["targets"] as? [String: Any])?[direction])
    }
}

final class Agent {
    let app: URL
    let state: URL
    let config: URL
    private(set) var launches = 0
    private(set) var kills = 0

    init(app: URL, state: URL, config: URL) {
        self.app = app
        self.state = state
        self.config = config
    }

    var cli: String { app.appendingPathComponent("Contents/MacOS/spaceman").path }
    private var agentExecutable: String { app.appendingPathComponent("Contents/MacOS/spaceman-agent").path }

    var environment: [String: String] {
        [
            "SPACEMAN_APPLICATION_SUPPORT": state.path,
            "SPACEMAN_CONFIG": config.path,
        ]
    }

    /// Launches through LaunchServices, so Accessibility is checked against Spaceman.app itself
    /// rather than whatever shell started the gauntlet.
    func launch() -> String? {
        var arguments = ["-n", "-a", app.path]
        for (key, value) in environment.sorted(by: { $0.key < $1.key }) {
            arguments += ["--env", "\(key)=\(value)"]
        }
        let result = run("/usr/bin/open", arguments)
        if result.status != 0 { return "open failed: \(result.err)" }
        launches += 1
        return nil
    }

    var pid: pid_t? {
        let result = run("/usr/bin/pgrep", ["-f", agentExecutable])
        return result.out.split(separator: "\n").compactMap { pid_t($0) }.first
    }

    func hardKill() {
        guard let pid else { return }
        kill(pid, SIGKILL)
        kills += 1
    }

    func ctl(_ arguments: [String]) -> (ok: Bool, out: String) {
        let result = run(cli, ["ctl"] + arguments, environment: environment)
        return (result.status == 0, result.status == 0 ? result.out : result.err)
    }

    func json(_ arguments: [String]) -> [String: Any]? {
        let result = ctl(arguments)
        guard result.ok, let data = result.out.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    func debug() -> Debug? { json(["debug"]).map(Debug.init) }
    func status() -> [String: Any]? { json(["status"]) }

    var parkedFile: URL { state.appendingPathComponent("parked-windows.json") }

    /// `parked-windows.json`: `[[window, frame], ...]`. A missing file is no parked windows;
    /// a malformed one is an error, never silently empty.
    func parked() -> Result<[UInt32: Rect], Problem> {
        guard let data = try? Data(contentsOf: parkedFile) else { return .success([:]) }
        guard let entries = (try? JSONSerialization.jsonObject(with: data)) as? [[Any]] else {
            return .failure(Problem("parked-windows.json is not an array of [window, frame] pairs"))
        }
        var result: [UInt32: Rect] = [:]
        for entry in entries {
            guard entry.count == 2, let id = windowId(entry[0]), let frame = Rect(json: entry[1]) else {
                return .failure(Problem("parked-windows.json has a malformed entry"))
            }
            result[id] = frame
        }
        return .success(result)
    }

    func doctor() -> (status: Int32, out: String) {
        let result = run(cli, ["doctor"], environment: environment)
        return (result.status, result.out + result.err)
    }

    var version: String {
        let plist = app.appendingPathComponent("Contents/Info.plist")
        let info = NSDictionary(contentsOf: plist)
        return info?["CFBundleShortVersionString"] as? String ?? "unknown"
    }
}
