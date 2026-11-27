#!/usr/bin/env swift
// Rasterises Fractile's app icon (brand/fractile-icon.svg) at every size macOS asks for and packs
// them into an .icns with iconutil. scripts/build-app.sh uses it when Xcode's actool isn't there
// to compile app/AppIcon.icon. macOS draws the SVG itself, so the icon has one source.
//
//   swift scripts/make-icns.swift <icon.svg> <output.icns>

import AppKit

let arguments = CommandLine.arguments.dropFirst()
guard arguments.count == 2, let image = NSImage(contentsOfFile: arguments.first!) else {
    FileHandle.standardError.write("usage: make-icns.swift <icon.svg> <output.icns>\n".data(using: .utf8)!)
    exit(2)
}
let output = arguments.last!
let iconset = URL(fileURLWithPath: NSTemporaryDirectory())
    .appendingPathComponent("Fractile-\(ProcessInfo.processInfo.processIdentifier).iconset")
try? FileManager.default.removeItem(at: iconset)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

func png(pixels: Int) -> Data {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high
    image.draw(in: NSRect(x: 0, y: 0, width: pixels, height: pixels))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = scale == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
        try png(pixels: points * scale).write(to: iconset.appendingPathComponent(name))
    }
}
let pack = Process()
pack.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
pack.arguments = ["-c", "icns", iconset.path, "-o", output]
try pack.run()
pack.waitUntilExit()
try? FileManager.default.removeItem(at: iconset)
exit(pack.terminationStatus)
