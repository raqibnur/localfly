import LocalflyCore
import SwiftUI

struct AppsView: View {
    @EnvironmentObject private var model: AppModel
    @State private var editing: SavedApp?
    @State private var adding: SavedApp?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                header
                if model.apps.isEmpty {
                    EmptyApps(add: { adding = SavedApp(name: "", port: $0 ?? 3000) })
                } else {
                    VStack(spacing: 10) {
                        ForEach(model.apps) { app in
                            AppCard(app: app, edit: { editing = app })
                                .transition(.asymmetric(insertion: .scale(scale: 0.97).combined(with: .opacity), removal: .opacity))
                        }
                    }
                }
                if !model.detectedPorts.isEmpty && !model.apps.isEmpty {
                    DetectedPorts(ports: model.detectedPorts) { adding = SavedApp(name: "", port: $0) }
                }
            }
            .padding(.horizontal, 28)
            .padding(.top, 8)
            .padding(.bottom, 28)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity)
        }
        .sheet(item: $adding) { draft in AppEditor(draft: draft, isNew: true) }
        .sheet(item: $editing) { app in AppEditor(draft: app, isNew: false) }
        .onReceive(model.$wantsAddApp) { wants in
            guard wants else { return }
            model.wantsAddApp = false
            adding = SavedApp(name: "", port: model.detectedPorts.first ?? 3000)
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button { adding = SavedApp(name: "", port: model.detectedPorts.first ?? 3000) } label: {
                    Label("Add App", systemImage: "plus")
                }
                .help("Add an app (⌘N)")
                .keyboardShortcut("n")
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Apps")
                .font(.system(size: 30, weight: .bold))
            HStack(spacing: 6) {
                Text(summary).foregroundStyle(.secondary)
                if model.liveCount > 0 { Pill(text: "\(model.liveCount) live", dot: .live, tint: .live) }
                let idle = model.apps.count - model.liveCount
                if idle > 0 && !model.apps.isEmpty { Pill(text: "\(idle) off") }
                Text("via").foregroundStyle(.secondary)
                HStack(spacing: 5) {
                    SignalBars(latency: model.relayLatency, reachable: model.relayReachable)
                    Text(model.relayHost)
                    Text(Format.latency(model.relayLatency)).foregroundStyle(.secondary)
                }
                .font(.system(size: 11.5, weight: .semibold))
                .padding(.horizontal, 9).padding(.vertical, 4)
                .background(Capsule().fill(Color.primary.opacity(0.06)))
            }
            .font(.system(size: 13))
        }
    }

    private var summary: String {
        switch model.apps.count {
        case 0: "Nothing saved yet."
        case 1: "One app saved:"
        default: "\(model.apps.count) apps saved:"
        }
    }
}

// MARK: Card

struct AppCard: View {
    @EnvironmentObject private var model: AppModel
    let app: SavedApp
    let edit: () -> Void
    @State private var hovering = false
    @State private var confirmDelete = false

    private var tunnel: TunnelInfo? { model.tunnel(for: app) }
    private var portListening: Bool? { model.listening[app.port] }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 14) {
                Monogram(app: app)
                    .overlay(alignment: .bottomTrailing) {
                        if let tunnel, tunnel.status != .stopped {
                            StatusDot(color: tunnel.status.tint, pulsing: tunnel.status == .live, size: 9)
                                .padding(2)
                                .background(Circle().fill(.background))
                                .offset(x: 4, y: 4)
                                .transition(.scale.combined(with: .opacity))
                        }
                    }
                VStack(alignment: .leading, spacing: 3) {
                    Text(app.name).font(.system(size: 14, weight: .semibold)).lineLimit(1)
                    HStack(spacing: 5) {
                        Circle()
                            .fill(portListening == true ? Color.live : portListening == false ? Color.secondary.opacity(0.5) : .clear)
                            .frame(width: 5, height: 5)
                        Text(verbatim: "localhost:\(String(app.port))")
                            .font(.system(size: 11.5, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }
                    .help(portListening == true ? "Something is listening on :\(String(app.port))" : "Nothing is listening on :\(String(app.port))")
                }
                .frame(minWidth: 120, alignment: .leading)

                Spacer(minLength: 12)
                trailing
                actionsMenu
                Toggle("Expose \(app.name)", isOn: Binding(get: { model.isOn(app) }, set: { model.setOn($0, for: app) }))
                    .toggleStyle(.switch)
                    .labelsHidden()
                    .tint(.flare)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)

            if let tunnel, tunnel.status == .live {
                LiveStats(tunnel: tunnel)
                    .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .surface(highlighted: hovering)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.15), value: hovering)
        .contextMenu { menuItems }
        .confirmationDialog("Delete \(app.name)?", isPresented: $confirmDelete) {
            Button("Delete", role: .destructive) { model.delete(app) }
        } message: {
            Text("Its tunnel closes and the app is removed from Localfly. Nothing on localhost changes.")
        }
    }

    @ViewBuilder private var trailing: some View {
        if let notice = model.notices[app.id] {
            Label(notice, systemImage: "moon.zzz")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(Color.pending)
                .transition(.opacity)
        } else if model.starting.contains(app.id) {
            progress("Checking :\(String(app.port))…")
        } else if let tunnel {
            switch tunnel.status {
            case .live:
                URLChip(app: app, url: model.displayAddress(app) ?? "", secure: app.publicAddress?.hasPrefix("https://") == true)
                    .transition(.scale(scale: 0.92).combined(with: .opacity))
            case .connecting:
                progress(tunnel.retryAttempt > 0 ? "Reconnecting…" : "Connecting…")
            case .error(let reason):
                Label(reason, systemImage: "exclamationmark.triangle.fill")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(Color.fault)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .frame(maxWidth: 260, alignment: .trailing)
                    .help(reason)
            case .stopped:
                EmptyView()
            }
        }
    }

    private func progress(_ text: String) -> some View {
        HStack(spacing: 7) {
            ProgressView().controlSize(.small).scaleEffect(0.7).frame(width: 14, height: 14)
            Text(text)
        }
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(.secondary)
    }

    private var actionsMenu: some View {
        Menu { menuItems } label: {
            Image(systemName: "ellipsis")
                .font(.system(size: 12, weight: .semibold))
                .frame(width: 22, height: 22)
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .tint(.secondary)
        .foregroundStyle(.secondary)
        .fixedSize()
        .opacity(hovering ? 1 : 0.35)
        .help("More")
    }

    @ViewBuilder private var menuItems: some View {
        if tunnel?.status == .live {
            Button("Copy Public URL") { model.copyURL(app) }
            Button("Open in Browser") { model.openInBrowser(app) }
            Divider()
        }
        Button("Edit…", action: edit)
        Toggle("Start on Launch", isOn: Binding(
            get: { app.startOnLaunch },
            set: { var copy = app; copy.startOnLaunch = $0; model.update(copy) }
        ))
        Divider()
        Button("Delete…", role: .destructive) { confirmDelete = true }
    }
}

/// The public address with copy and open actions.
struct URLChip: View {
    @EnvironmentObject private var model: AppModel
    let app: SavedApp
    let url: String
    var secure = false

    var body: some View {
        HStack(spacing: 2) {
            if secure {
                Image(systemName: "lock.fill")
                    .font(.system(size: 9, weight: .bold))
                    .foregroundStyle(Color.live)
                    .padding(.leading, 9)
                    .padding(.trailing, 3)
                    .help("Served over HTTPS by your relay")
            }
            Text(url)
                .font(.system(size: 12, weight: .medium, design: .monospaced))
                .textSelection(.enabled)
                .padding(.leading, secure ? 0 : 10)
                .padding(.trailing, 4)
            CopyButton(copied: model.recentlyCopied(app)) {
                model.copyURL(app)
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { model.objectWillChange.send() }
            }
            Button { model.openInBrowser(app) } label: {
                Image(systemName: "arrow.up.right")
                    .font(.system(size: 11, weight: .semibold))
                    .frame(width: 22, height: 22)
                    .foregroundStyle(.secondary)
                    .contentShape(Rectangle())
            }
            .buttonStyle(PressableStyle())
            .help("Open in browser")
        }
        .padding(3)
        .background(Capsule().fill(Color.live.opacity(0.1)))
        .overlay(Capsule().strokeBorder(Color.live.opacity(0.25), lineWidth: 1))
    }
}

/// Uptime and traffic strip under a live card.
struct LiveStats: View {
    let tunnel: TunnelInfo

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            HStack(spacing: 18) {
                if let since = tunnel.liveSince {
                    stat("clock", Format.uptime(since: since, now: context.date), "Live for")
                }
                stat("person.2", "\(tunnel.stats.activeConnections) open · \(tunnel.stats.totalConnections) total", "Visitor connections")
                stat("arrow.down", Format.bytes(tunnel.stats.bytesIn), "Received from visitors")
                stat("arrow.up", Format.bytes(tunnel.stats.bytesOut), "Sent to visitors")
                Spacer()
            }
        }
        .font(.system(size: 11.5).monospacedDigit())
        .foregroundStyle(.secondary)
        .padding(.horizontal, 16)
        .padding(.vertical, 9)
        .background(Color.primary.opacity(0.025))
        .overlay(alignment: .top) { Rectangle().fill(Color.primary.opacity(0.06)).frame(height: 1) }
    }

    private func stat(_ symbol: String, _ value: String, _ help: String) -> some View {
        HStack(spacing: 5) {
            Image(systemName: symbol).font(.system(size: 10, weight: .semibold)).foregroundStyle(.tertiary)
            Text(value)
        }
        .help(help)
    }
}

// MARK: Empty & detected

struct EmptyApps: View {
    @EnvironmentObject private var model: AppModel
    let add: (UInt16?) -> Void

    var body: some View {
        VStack(spacing: 16) {
            ZStack {
                Circle().strokeBorder(style: StrokeStyle(lineWidth: 1.2, dash: [3, 5])).foregroundStyle(.tertiary).frame(width: 112, height: 112)
                Circle().fill(Color.flare.opacity(0.08)).frame(width: 76, height: 76)
                BrandMark(size: 44)
            }
            VStack(spacing: 5) {
                Text("Put a local app on the internet").font(.system(size: 17, weight: .semibold))
                Text("Save an app by its port, flip the switch, and share the address.\nWorks from any phone or network.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }
            Button("Add Your First App") { add(nil) }
                .buttonStyle(.borderedProminent)
                .tint(.flare)
                .controlSize(.large)
            if !model.detectedPorts.isEmpty {
                VStack(spacing: 8) {
                    Text("Running on this Mac right now").font(.caption).foregroundStyle(.secondary)
                    HStack(spacing: 6) {
                        ForEach(model.detectedPorts, id: \.self) { port in
                            Button { add(port) } label: { Label(":\(String(port))", systemImage: "plus") }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                        }
                    }
                }
                .padding(.top, 6)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 48)
        .surface(radius: 18)
    }
}

struct DetectedPorts: View {
    let ports: [UInt16]
    let add: (UInt16) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("ALSO RUNNING ON THIS MAC")
                .font(.system(size: 10.5, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                ForEach(ports, id: \.self) { port in
                    Button { add(port) } label: {
                        HStack(spacing: 6) {
                            StatusDot(color: .live, size: 6)
                            Text(verbatim: "localhost:\(String(port))").font(.system(size: 12, design: .monospaced))
                            Image(systemName: "plus").font(.system(size: 10, weight: .bold)).foregroundStyle(.secondary)
                        }
                        .padding(.horizontal, 11).padding(.vertical, 7)
                        .surface(radius: 9)
                    }
                    .buttonStyle(PressableStyle(hoverFill: false))
                    .help("Save localhost:\(String(port)) as an app")
                }
            }
        }
    }
}
