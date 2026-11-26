// Fractile live gauntlet. Built and run by scripts/gauntlet.sh; see docs/development.md.
import Foundation

let usage = """
    usage: gauntlet run --app Fractile.app --out DIR [--only boot,switch,focus,reflow,float,scratchpad,crash]
                        [--reflow-budget-ms N]
           gauntlet windows            window-server ground truth as JSON (titles never read)
           gauntlet window --label L --dir DIR --title T    one disposable test window (internal)
    """

func value(_ arguments: [String], _ flag: String) -> String? {
    guard let index = arguments.firstIndex(of: flag), index + 1 < arguments.count else { return nil }
    return arguments[index + 1]
}

func fail(_ message: String, code: Int32 = 2) -> Never {
    FileHandle.standardError.write(Data("\(message)\n".utf8))
    exit(code)
}

let arguments = Array(CommandLine.arguments.dropFirst())
switch arguments.first {
case "window":
    guard let label = value(arguments, "--label"), let directory = value(arguments, "--dir"),
        let title = value(arguments, "--title")
    else { fail(usage) }
    runWindowMode(label: label, directory: URL(fileURLWithPath: directory), title: title)

case "windows":
    let windows = Native.windows().values.filter { $0.layer == 0 }.sorted { $0.id < $1.id }
    writeJSON(
        ["frontmost_pid": Native.frontmostPid() ?? 0, "displays": Native.displays().map(\.json), "windows": windows.map(\.json)],
        to: URL(fileURLWithPath: "/dev/stdout"))
    print("")

case "run":
    guard let app = value(arguments, "--app"), let out = value(arguments, "--out") else { fail(usage) }
    let bundle = Bundle.main.bundleURL
    guard bundle.pathExtension == "app" else { fail("run gauntlet from the FractileGauntlet.app that scripts/gauntlet.sh builds") }
    let appURL = URL(fileURLWithPath: app).standardizedFileURL
    for binary in ["fractile", "fractile-agent"] {
        guard FileManager.default.isExecutableFile(atPath: appURL.appendingPathComponent("Contents/MacOS/\(binary)").path) else {
            fail("\(app) has no Contents/MacOS/\(binary); build it with scripts/build-app.sh")
        }
    }
    // Two window managers fight over frames, and the user's own Fractile must never be touched.
    for name in ["fractile-agent", "AeroSpace", "yabai", "Amethyst"] {
        if run("/usr/bin/pgrep", ["-x", name]).status == 0 {
            fail("\(name) is running. Quit it first: the gauntlet runs its own Fractile and must be the only window manager.")
        }
    }
    var options = Options(app: appURL, out: URL(fileURLWithPath: out).standardizedFileURL)
    if let only = value(arguments, "--only") { options.only = Set(only.split(separator: ",").map(String.init)) }
    if let budget = value(arguments, "--reflow-budget-ms").flatMap(Double.init) { options.reflowBudgetMs = budget }

    let gauntlet: Gauntlet
    do {
        gauntlet = try Gauntlet(options: options, bundle: bundle)
    } catch {
        fail("cannot prepare \(out): \(error)")
    }
    for signalNumber in [SIGINT, SIGTERM] {
        signal(signalNumber, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: signalNumber, queue: .main)
        source.setEventHandler {
            print("interrupted: restoring and closing test windows")
            gauntlet.cleanup()
            exit(130)
        }
        source.resume()
        _ = Unmanaged.passRetained(source)
    }

    print("Fractile gauntlet · Fractile \(gauntlet.agent.version) · state \(gauntlet.agent.state.path)")
    let scenarios: [(String, () throws -> Void)] = [
        ("switch", gauntlet.switching), ("focus", gauntlet.focus), ("reflow", gauntlet.reflow),
        ("float", gauntlet.float), ("scratchpad", gauntlet.scratchpad), ("crash", gauntlet.crash),
    ]
    var aborted = false
    do { try gauntlet.boot() } catch { aborted = true }
    for (name, body) in scenarios where gauntlet.selected(name) {
        gauntlet.scenario = name
        if aborted {
            gauntlet.skip("all steps", "an earlier step failed")
            continue
        }
        do { try body() } catch { aborted = true }
    }
    gauntlet.recovery()
    gauntlet.cleanup()
    gauntlet.writeReceipt(artifacts: ["state/agent.log", "state/journal.jsonl", "state/parked-windows.json", "state/workspace-state.json", "doctor.txt", "snapshots/"])
    exit(gauntlet.failed ? 1 : 0)

default:
    fail(usage)
}
