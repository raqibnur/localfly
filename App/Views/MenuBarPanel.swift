import LocalflyCore
import SwiftUI

/// Popover shown from the menu bar icon.
struct MenuBarPanel: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(spacing: 0) {
            header
                .padding(.horizontal, 14)
                .padding(.top, 14)
                .padding(.bottom, 10)

            if model.apps.isEmpty {
                VStack(spacing: 8) {
                    Text("No apps yet").font(.system(size: 13, weight: .semibold))
                    Text("Save a port to expose it in one click.").font(.system(size: 12)).foregroundStyle(.secondary)
                    Button("Add App…") { showMain(addApp: true) }
                        .buttonStyle(.borderedProminent).tint(.flare).controlSize(.small)
                        .padding(.top, 2)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 22)
                .surface(radius: 12)
                .padding(.horizontal, 10)
            } else {
                VStack(spacing: 2) {
                    ForEach(model.apps) { app in MenuAppRow(app: app) }
                }
                .padding(.horizontal, 6)
            }

            Divider().padding(.top, 10).padding(.horizontal, 14)

            VStack(spacing: 0) {
                MenuAction(title: "Open Localfly", symbol: "macwindow", shortcut: "O") { showMain() }
                    .keyboardShortcut("o")
                MenuAction(title: "Quit Localfly", symbol: "power", shortcut: "Q") { NSApp.terminate(nil) }
                    .keyboardShortcut("q")
            }
            .padding(6)
        }
        .frame(width: 340)
        .tint(.flare)
    }

    private var header: some View {
        HStack(spacing: 10) {
            BrandMark(size: 26)
            VStack(alignment: .leading, spacing: 1) {
                Text("Localfly").font(.system(size: 13, weight: .bold))
                HStack(spacing: 5) {
                    SignalBars(latency: model.relayLatency, reachable: model.relayReachable).scaleEffect(0.8)
                    Text("\(model.relayHost) · \(Format.latency(model.relayLatency))")
                }
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            }
            Spacer()
            if model.liveCount > 0 { Pill(text: "\(model.liveCount) live", dot: .live, tint: .live) }
        }
    }

    private func showMain(addApp: Bool = false) {
        openWindow(id: "main")
        NSApp.activate(ignoringOtherApps: true)
        if addApp { model.wantsAddApp = true }
    }
}

struct MenuAppRow: View {
    @EnvironmentObject private var model: AppModel
    let app: SavedApp
    @State private var hovering = false

    private var tunnel: TunnelInfo? { model.tunnel(for: app) }

    var body: some View {
        HStack(spacing: 10) {
            Monogram(app: app, size: 28)
                .overlay(alignment: .bottomTrailing) {
                    if let tunnel {
                        StatusDot(color: tunnel.status.tint, pulsing: tunnel.status == .live, size: 7)
                            .padding(1.5)
                            .background(Circle().fill(.background))
                            .offset(x: 3, y: 3)
                    }
                }
            VStack(alignment: .leading, spacing: 1) {
                Text(app.name).font(.system(size: 12.5, weight: .semibold)).lineLimit(1)
                Text(subtitle)
                    .font(.system(size: 11, design: tunnel?.status == .live ? .monospaced : .default))
                    .foregroundStyle(subtitleColor)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            Spacer(minLength: 6)
            if tunnel?.status == .live {
                CopyButton(copied: model.recentlyCopied(app)) {
                    model.copyURL(app)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { model.objectWillChange.send() }
                }
            }
            Toggle("Expose \(app.name)", isOn: Binding(get: { model.isOn(app) }, set: { model.setOn($0, for: app) }))
                .toggleStyle(.switch)
                .controlSize(.mini)
                .labelsHidden()
                .tint(.flare)
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.primary.opacity(hovering ? 0.06 : 0)))
        .onHover { hovering = $0 }
    }

    private var subtitle: String {
        if let notice = model.notices[app.id] { return notice }
        if model.starting.contains(app.id) { return "Checking :\(String(app.port))…" }
        switch tunnel?.status {
        case .live?: return model.displayAddress(app) ?? ""
        case .connecting?: return "Connecting…"
        case .error(let reason)?: return reason
        default: return "localhost:\(String(app.port))"
        }
    }

    private var subtitleColor: Color {
        if model.notices[app.id] != nil { return .pending }
        if case .error? = tunnel?.status { return .fault }
        return .secondary
    }
}

struct MenuAction: View {
    let title: String
    let symbol: String
    let shortcut: String
    let action: () -> Void
    @State private var hovering = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                Image(systemName: symbol).font(.system(size: 12)).frame(width: 16)
                Text(title).font(.system(size: 12.5))
                Spacer()
                Text("⌘\(shortcut)").font(.system(size: 11)).foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .contentShape(Rectangle())
            .background(RoundedRectangle(cornerRadius: 6, style: .continuous).fill(Color.primary.opacity(hovering ? 0.08 : 0)))
        }
        .buttonStyle(.plain)
        .onHover { hovering = $0 }
    }
}
