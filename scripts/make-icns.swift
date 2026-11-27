#!/usr/bin/env swift
// Draws Fractile's app icon (the F mark in ink on its paper tile) at every size macOS asks for
// and packs them into an .icns with iconutil. scripts/build-app.sh uses it when Xcode's actool
// isn't there to compile app/AppIcon.icon. The mark's paths are brand/fractile-mark.svg's.
//
//   swift scripts/make-icns.swift <output.icns>

import AppKit

let output = CommandLine.arguments.dropFirst().first ?? "target/AppIcon.icns"
let iconset = URL(fileURLWithPath: NSTemporaryDirectory())
    .appendingPathComponent("Fractile-\(ProcessInfo.processInfo.processIdentifier).iconset")
try? FileManager.default.removeItem(at: iconset)
try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)

func color(_ hex: UInt32) -> CGColor {
    CGColor(
        srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
        green: CGFloat((hex >> 8) & 0xff) / 255,
        blue: CGFloat(hex & 0xff) / 255,
        alpha: 1
    )
}

/// The mark's two shapes in brand/fractile-mark.svg's 120 x 116 space, y down.
let shapes: [[CGPoint]] = [
    [(3, 113), (3, 58), (42, 3), (117, 3), (101, 26), (54, 26), (35, 52), (35, 113)],
    [(44, 50), (115, 50), (99, 73), (40, 73), (35, 80), (35, 64)],
].map { $0.map { CGPoint(x: $0.0, y: $0.1) } }

func draw(pixels: Int) -> Data {
    let size = CGFloat(pixels)
    let context = CGContext(
        data: nil, width: pixels, height: pixels, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    )!
    let unit = size / 1024
    // macOS's icon grid: an 824 pt tile centred on 1024, its corners at 185.
    let tile = CGRect(x: 100 * unit, y: 100 * unit, width: 824 * unit, height: 824 * unit)
    let tilePath = CGPath(
        roundedRect: tile, cornerWidth: 185 * unit, cornerHeight: 185 * unit, transform: nil)
    context.saveGState()
    context.setShadow(
        offset: CGSize(width: 0, height: -10 * unit), blur: 24 * unit,
        color: CGColor(gray: 0, alpha: 0.28))
    context.addPath(tilePath)
    context.setFillColor(color(0xEEECE6))
    context.fillPath()
    context.restoreGState()
    context.saveGState()
    context.addPath(tilePath)
    context.clip()
    let gradient = CGGradient(
        colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
        colors: [color(0xF6F5F1), color(0xE4E2DA)] as CFArray, locations: [0, 1])!
    context.drawLinearGradient(
        gradient, start: CGPoint(x: 0, y: tile.maxY), end: CGPoint(x: 0, y: tile.minY),
        options: [])
    context.restoreGState()
    // The mark, 480 pt across, centred on the tile; the SVG's y runs down, the context's up.
    let scale = 480 * unit / 114
    let origin = CGPoint(x: 512 * unit - 60 * scale, y: 512 * unit + 58 * scale)
    context.setFillColor(color(0x111111))
    context.setStrokeColor(color(0x111111))
    context.setLineWidth(3 * scale)
    context.setLineJoin(.round)
    for shape in shapes {
        context.beginPath()
        for (index, point) in shape.enumerated() {
            let at = CGPoint(x: origin.x + point.x * scale, y: origin.y - point.y * scale)
            if index == 0 { context.move(to: at) } else { context.addLine(to: at) }
        }
        context.closePath()
        context.drawPath(using: .fillStroke)
    }
    let image = context.makeImage()!
    return NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:])!
}

for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = scale == 1 ? "icon_\(points)x\(points).png" : "icon_\(points)x\(points)@2x.png"
        try draw(pixels: points * scale).write(to: iconset.appendingPathComponent(name))
    }
}
let pack = Process()
pack.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
pack.arguments = ["-c", "icns", iconset.path, "-o", output]
try pack.run()
pack.waitUntilExit()
try? FileManager.default.removeItem(at: iconset)
exit(pack.terminationStatus)
