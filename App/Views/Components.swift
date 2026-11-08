import LocalflyCore
import SwiftUI

// MARK: Palette

extension Color {
    /// Localfly's signal colour: a warm coral used for the toggle, selection and brand mark.
    static let flare = Color(red: 1.0, green: 0.38, blue: 0.29)
    static let flareDeep = Color(red: 0.93, green: 0.22, blue: 0.33)
    static let live = Color(red: 0.19, green: 0.80, blue: 0.47)
    static let pending = Color(red: 0.98, green: 0.69, blue: 0.18)
    static let fault = Color(red: 0.96, green: 0.30, blue: 0.30)
}

extension TunnelStatus {
    var tint: Color {
        switch self {
        case .live: .live
        case .connecting: .pending
        case .error: .fault
        case .stopped: .secondary
        }
    }
}

// MARK: Surfaces

/// Quiet raised surface that reads in both light and dark appearance.
struct Surface: ViewModifier {
    var radius: CGFloat = 14
    var highlighted = false

    func body(content: Content) -> some View {
        content
            .background {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .fill(Color.primary.opacity(highlighted ? 0.065 : 0.04))
            }
            .overlay {
                RoundedRectangle(cornerRadius: radius, style: .continuous)
                    .strokeBorder(Color.primary.opacity(0.07), lineWidth: 1)
            }
    }
}

extension View {
    func surface(radius: CGFloat = 14, highlighted: Bool = false) -> some View {
        modifier(Surface(radius: radius, highlighted: highlighted))
    }
}

// MARK: Brand

struct BrandMark: View {
    var size: CGFloat = 28

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
            .fill(LinearGradient(colors: [.flare, .flareDeep], startPoint: .topLeading, endPoint: .bottomTrailing))
            .overlay {
                Image(systemName: "paperplane.fill")
                    .font(.system(size: size * 0.48, weight: .semibold))
                    .foregroundStyle(.white)
                    .rotationEffect(.degrees(-8))
                    .offset(x: -size * 0.02, y: size * 0.02)
            }
            .overlay {
                RoundedRectangle(cornerRadius: size * 0.28, style: .continuous)
                    .strokeBorder(.white.opacity(0.18), lineWidth: 0.5)
            }
            .frame(width: size, height: size)
            .shadow(color: .flareDeep.opacity(0.25), radius: size * 0.12, y: size * 0.05)
    }
}

/// Coloured initial standing in for an app icon.
struct Monogram: View {
    let app: SavedApp
    var size: CGFloat = 38

    var body: some View {
        RoundedRectangle(cornerRadius: size * 0.3, style: .continuous)
            .fill(LinearGradient(
                colors: [Color(hue: app.hue, saturation: 0.55, brightness: 0.92), Color(hue: app.hue + 0.06, saturation: 0.7, brightness: 0.72)],
                startPoint: .topLeading, endPoint: .bottomTrailing
            ))
            .overlay {
                Text(app.initial)
                    .font(.system(size: size * 0.44, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
                    .shadow(color: .black.opacity(0.15), radius: 1, y: 0.5)
            }
            .frame(width: size, height: size)
    }
}

// MARK: Status

/// Status dot. Live tunnels emit a slow ring so they read as "on air" at a glance.
struct StatusDot: View {
    let color: Color
    var pulsing = false
    var size: CGFloat = 8
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var expanded = false

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
            .background {
                if pulsing && !reduceMotion {
                    Circle()
                        .stroke(color.opacity(expanded ? 0 : 0.6), lineWidth: 1.5)
                        .scaleEffect(expanded ? 2.6 : 1)
                        .onAppear {
                            withAnimation(.easeOut(duration: 1.6).repeatForever(autoreverses: false)) { expanded = true }
                        }
                        .onDisappear { expanded = false }
                }
            }
    }
}

/// Compact rounded label, e.g. "2 live".
struct Pill: View {
    let text: String
    var dot: Color?
    var tint: Color = .secondary

    var body: some View {
        HStack(spacing: 5) {
            if let dot { StatusDot(color: dot, size: 6) }
            Text(text)
        }
        .font(.system(size: 11.5, weight: .semibold))
        .foregroundStyle(tint)
        .padding(.horizontal, 9)
        .padding(.vertical, 4)
        .background(Capsule().fill((dot ?? tint).opacity(0.13)))
    }
}

/// Four-bar latency meter for the relay.
struct SignalBars: View {
    let latency: TimeInterval?
    var reachable: Bool?

    private var level: Int {
        guard let latency else { return 0 }
        switch latency {
        case ..<0.06: return 4
        case ..<0.12: return 3
        case ..<0.25: return 2
        default: return 1
        }
    }

    private var tint: Color { reachable == false ? .fault : level >= 3 ? .live : .pending }

    var body: some View {
        HStack(alignment: .bottom, spacing: 1.5) {
            ForEach(1...4, id: \.self) { bar in
                RoundedRectangle(cornerRadius: 1)
                    .fill(bar <= level ? tint : Color.primary.opacity(0.15))
                    .frame(width: 3, height: 3 + CGFloat(bar) * 2.2)
            }
        }
        .accessibilityLabel(latency.map { "Relay latency \(Int($0 * 1000)) milliseconds" } ?? "Relay unreachable")
    }
}

// MARK: Buttons

/// Copy button that confirms with a checkmark.
struct CopyButton: View {
    let copied: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack {
                Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    .id(copied)
                    .transition(.scale(scale: 0.6).combined(with: .opacity))
            }
            .font(.system(size: 11, weight: .semibold))
            .frame(width: 22, height: 22)
            .foregroundStyle(copied ? Color.live : .secondary)
            .contentShape(Rectangle())
        }
        .buttonStyle(PressableStyle())
        .animation(.ui, value: copied)
        .help("Copy public URL")
        .accessibilityLabel(copied ? "Copied" : "Copy public URL")
    }
}

/// Plain button that dips slightly on press and lights up on hover.
struct PressableStyle: ButtonStyle {
    var hoverFill = true

    func makeBody(configuration: Configuration) -> some View {
        HoverHost(hoverFill: hoverFill) {
            configuration.label
                .scaleEffect(configuration.isPressed ? 0.94 : 1)
                .animation(.ui, value: configuration.isPressed)
        }
    }

    private struct HoverHost<Label: View>: View {
        let hoverFill: Bool
        @ViewBuilder let label: Label
        @State private var hovering = false

        var body: some View {
            label
                .background {
                    if hoverFill {
                        RoundedRectangle(cornerRadius: 6, style: .continuous)
                            .fill(Color.primary.opacity(hovering ? 0.08 : 0))
                    }
                }
                .onHover { hovering = $0 }
                .animation(.easeOut(duration: 0.12), value: hovering)
        }
    }
}

// MARK: Formatting

enum Format {
    static func bytes(_ value: UInt64) -> String {
        if value == 0 { return "0 B" }
        return ByteCountFormatter.string(fromByteCount: Int64(value), countStyle: .file)
    }

    static func uptime(since date: Date, now: Date = Date()) -> String {
        let seconds = Int(now.timeIntervalSince(date))
        if seconds < 60 { return "\(seconds)s" }
        if seconds < 3600 { return "\(seconds / 60)m \(seconds % 60)s" }
        return "\(seconds / 3600)h \(seconds % 3600 / 60)m"
    }

    static func latency(_ value: TimeInterval?) -> String {
        value.map { "\(max(1, Int($0 * 1000))) ms" } ?? "offline"
    }
}

extension Animation {
    /// House spring: quick, settles without overshoot. (`.snappy` needs macOS 14.)
    static let ui = Animation.spring(response: 0.3, dampingFraction: 0.86)
}
