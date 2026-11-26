// Ground truth from the window server, independent of Fractile's own observation code.
import AppKit
import CoreGraphics
import Foundation

struct Rect: Equatable, CustomStringConvertible {
    var x: Double
    var y: Double
    var width: Double
    var height: Double

    init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    init(_ rect: CGRect) {
        self.init(x: rect.origin.x, y: rect.origin.y, width: rect.width, height: rect.height)
    }

    /// Fractile's JSON frames: `{"x", "y", "width", "height"}`.
    init?(json: Any?) {
        guard let object = json as? [String: Any],
            let x = number(object["x"]), let y = number(object["y"]),
            let width = number(object["width"]), let height = number(object["height"])
        else { return nil }
        self.init(x: x, y: y, width: width, height: height)
    }

    var maxX: Double { x + width }
    var maxY: Double { y + height }
    var area: Double { max(0, width) * max(0, height) }

    func intersection(_ other: Rect) -> Rect {
        let left = max(x, other.x)
        let top = max(y, other.y)
        let right = min(maxX, other.maxX)
        let bottom = min(maxY, other.maxY)
        return Rect(x: left, y: top, width: max(0, right - left), height: max(0, bottom - top))
    }

    func close(to other: Rect, tolerance: Double) -> Bool {
        abs(x - other.x) <= tolerance && abs(y - other.y) <= tolerance
            && abs(width - other.width) <= tolerance && abs(height - other.height) <= tolerance
    }

    func contains(_ other: Rect, tolerance: Double) -> Bool {
        other.x >= x - tolerance && other.y >= y - tolerance
            && other.maxX <= maxX + tolerance && other.maxY <= maxY + tolerance
    }

    var json: [String: Any] { ["x": x, "y": y, "width": width, "height": height] }

    var description: String {
        "\(Int(x.rounded())),\(Int(y.rounded())) \(Int(width.rounded()))x\(Int(height.rounded()))"
    }
}

struct Problem: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

func number(_ value: Any?) -> Double? { (value as? NSNumber)?.doubleValue }

struct NativeWindow {
    let id: UInt32
    let pid: pid_t
    let layer: Int
    let bounds: Rect
    let onScreen: Bool
    let alpha: Double

    var json: [String: Any] {
        ["id": id, "pid": pid, "layer": layer, "bounds": bounds.json, "on_screen": onScreen, "alpha": alpha]
    }
}

enum Native {
    /// Every window the window server knows, minimized and hidden ones included. Titles are never read.
    static func windows() -> [UInt32: NativeWindow] {
        var result: [UInt32: NativeWindow] = [:]
        for window in list([.optionAll, .excludeDesktopElements]) {
            result[window.id] = window
        }
        return result
    }

    /// Layer-0 windows on screen, front to back.
    static func frontToBack() -> [NativeWindow] {
        list([.optionOnScreenOnly, .excludeDesktopElements]).filter { $0.layer == 0 && $0.alpha > 0 }
    }

    /// The front layer-0 window that really shows on `display` (more than a parking strip).
    static func frontWindow(on display: Rect) -> NativeWindow? {
        frontToBack().first { window in
            let visible = window.bounds.intersection(display)
            return visible.width > 2 && visible.height > 2
        }
    }

    /// Full display bounds in global top-left coordinates, the space Accessibility frames use.
    static func displays() -> [Rect] {
        var count: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &count) == .success, count > 0 else { return [] }
        var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
        guard CGGetActiveDisplayList(count, &ids, &count) == .success else { return [] }
        return ids.prefix(Int(count)).map { Rect(CGDisplayBounds($0)) }
    }

    static func frontmostPid() -> pid_t? {
        NSWorkspace.shared.frontmostApplication?.processIdentifier
    }

    /// Lets AppKit deliver workspace notifications so `frontmostApplication` stays current.
    static func pump(_ seconds: Double) {
        RunLoop.current.run(until: Date(timeIntervalSinceNow: seconds))
    }

    private static func list(_ options: CGWindowListOption) -> [NativeWindow] {
        guard let raw = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
            return []
        }
        return raw.compactMap { info in
            guard let id = (info[kCGWindowNumber as String] as? NSNumber)?.uint32Value,
                let pid = (info[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value,
                let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                let rect = CGRect(dictionaryRepresentation: bounds as CFDictionary)
            else { return nil }
            return NativeWindow(
                id: id,
                pid: pid,
                layer: (info[kCGWindowLayer as String] as? NSNumber)?.intValue ?? 0,
                bounds: Rect(rect),
                onScreen: (info[kCGWindowIsOnscreen as String] as? NSNumber)?.boolValue ?? false,
                alpha: (info[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 1)
        }
    }
}

/// Runs a tool and returns its exit status and output. Output is small, so reading after exit is safe.
@discardableResult
func run(_ path: String, _ arguments: [String], environment: [String: String] = [:]) -> (status: Int32, out: String, err: String) {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: path)
    process.arguments = arguments
    process.environment = ProcessInfo.processInfo.environment.merging(environment) { _, new in new }
    let out = Pipe()
    let err = Pipe()
    process.standardOutput = out
    process.standardError = err
    do {
        try process.run()
    } catch {
        return (127, "", "\(error)")
    }
    let output = out.fileHandleForReading.readDataToEndOfFile()
    let errors = err.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    return (
        process.terminationStatus,
        String(decoding: output, as: UTF8.self),
        String(decoding: errors, as: UTF8.self)
    )
}

func processAlive(_ pid: pid_t) -> Bool { kill(pid, 0) == 0 || errno == EPERM }
