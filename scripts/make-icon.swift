// Renders the app icon: a coral squircle with a white paper plane.
// Usage: swift scripts/make-icon.swift App/Assets.xcassets/AppIcon.appiconset
import AppKit

let output = URL(fileURLWithPath: CommandLine.arguments[1])

func render(_ pixels: Int) -> Data {
    let size = CGFloat(pixels)
    let image = NSImage(size: NSSize(width: size, height: size), flipped: false) { _ in
        // macOS icon grid: 824/1024 body with soft shadow.
        let inset = size * 100 / 1024
        let body = NSRect(x: inset, y: inset * 1.12, width: size - inset * 2, height: size - inset * 2)
        let path = NSBezierPath(roundedRect: body, xRadius: body.width * 0.225, yRadius: body.width * 0.225)

        NSGraphicsContext.saveGraphicsState()
        let shadow = NSShadow()
        shadow.shadowColor = NSColor.black.withAlphaComponent(0.28)
        shadow.shadowBlurRadius = size * 0.03
        shadow.shadowOffset = NSSize(width: 0, height: -size * 0.012)
        shadow.set()
        NSColor(red: 0.93, green: 0.22, blue: 0.33, alpha: 1).setFill()
        path.fill()
        NSGraphicsContext.restoreGraphicsState()

        NSGradient(colors: [
            NSColor(red: 1.0, green: 0.52, blue: 0.32, alpha: 1),
            NSColor(red: 1.0, green: 0.36, blue: 0.29, alpha: 1),
            NSColor(red: 0.90, green: 0.18, blue: 0.36, alpha: 1),
        ])!.draw(in: path, angle: -60)

        // Soft top sheen.
        NSGraphicsContext.saveGraphicsState()
        path.addClip()
        NSGradient(colors: [NSColor.white.withAlphaComponent(0.22), NSColor.white.withAlphaComponent(0)])!
            .draw(in: NSRect(x: body.minX, y: body.midY, width: body.width, height: body.height / 2), angle: -90)
        NSGraphicsContext.restoreGraphicsState()

        // Dotted flight path behind the plane.
        let trail = NSBezierPath()
        trail.move(to: NSPoint(x: body.minX + body.width * 0.18, y: body.minY + body.height * 0.22))
        trail.curve(to: NSPoint(x: body.minX + body.width * 0.46, y: body.minY + body.height * 0.44),
                    controlPoint1: NSPoint(x: body.minX + body.width * 0.34, y: body.minY + body.height * 0.18),
                    controlPoint2: NSPoint(x: body.minX + body.width * 0.30, y: body.minY + body.height * 0.40))
        trail.lineWidth = size * 0.022
        trail.lineCapStyle = .round
        trail.setLineDash([0, size * 0.05], count: 2, phase: 0)
        NSColor.white.withAlphaComponent(0.55).setStroke()
        trail.stroke()

        let config = NSImage.SymbolConfiguration(pointSize: size * 0.40, weight: .semibold)
            .applying(.init(paletteColors: [.white]))
        if let plane = NSImage(systemSymbolName: "paperplane.fill", accessibilityDescription: nil)?.withSymbolConfiguration(config) {
            let planeSize = plane.size
            let origin = NSPoint(x: body.midX - planeSize.width / 2 + size * 0.015, y: body.midY - planeSize.height / 2 + size * 0.005)
            plane.draw(in: NSRect(origin: origin, size: planeSize))
        }
        return true
    }
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels, bitsPerSample: 8,
                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    rep.size = NSSize(width: size, height: size)
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    image.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

var images: [[String: String]] = []
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let name = "icon_\(points)x\(points)\(scale == 2 ? "@2x" : "").png"
        try! render(points * scale).write(to: output.appendingPathComponent(name))
        images.append(["idiom": "mac", "size": "\(points)x\(points)", "scale": "\(scale)x", "filename": name])
    }
}
let contents: [String: Any] = ["images": images, "info": ["author": "xcode", "version": 1]]
try! JSONSerialization.data(withJSONObject: contents, options: [.prettyPrinted, .sortedKeys])
    .write(to: output.appendingPathComponent("Contents.json"))
print("Wrote \(images.count) icon sizes")
