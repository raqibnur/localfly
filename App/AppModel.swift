import AppKit
import LocalflyCore
import ServiceManagement
import SwiftUI

/// UI state for the whole app. Tunnel logic lives in `TunnelManager`; this layer maps saved apps onto tunnels.
@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var apps: [SavedApp] = AppStore.load()
    /// Current tunnel per saved app. Absent means off.
    @Published private(set) var tunnels: [SavedApp.ID: TunnelInfo] = [:]
    /// Whether something listens on each saved port. Absent means not checked yet.
    @Published private(set) var listening: [UInt16: Bool] = [:]
    /// Why the last start attempt was refused, e.g. nothing running on the port.
    @Published private(set) var notices: [SavedApp.ID: String] = [:]
    /// Common dev-server ports that are listening but not saved yet.
    @Published private(set) var detectedPorts: [UInt16] = []
    @Published private(set) var relayLatency: TimeInterval?
    @Published private(set) var relayReachable: Bool?
    @Published private(set) var launchAtLogin = SMAppService.mainApp.status == .enabled
    @Published var launchAtLoginError: String?

    @AppStorage("relay") var relay = "bore.pub" { didSet { Task { await refreshRelay() } } }
    @AppStorage("autoReconnect") var autoReconnect = true
    @AppStorage("showDockIcon") var showDockIcon = false { didSet { applyActivationPolicy() } }
    @AppStorage("checkForUpdates") var checkForUpdates = true { didSet { if checkForUpdates { Task { await refreshUpdate() } } else { availableUpdate = nil } } }
    /// A newer GitHub release, if one exists.
    @Published private(set) var availableUpdate: UpdateChecker.Release?
    @Published var secret = SecretStore.read() { didSet { SecretStore.write(secret) } }

    /// Request from the menu bar to open the add-app sheet in the main window.
    @Published var wantsAddApp = false

    /// Apps whose port is being checked before the tunnel opens.
    @Published private(set) var starting: Set<SavedApp.ID> = []

    /// Tunnels are keyed by the saved app's id.
    private let manager = TunnelManager()
    private var copiedAt: [SavedApp.ID: Date] = [:]

    private static let commonPorts: [UInt16] = [3000, 3001, 3333, 4000, 4200, 4321, 5173, 5174, 8000, 8080, 8081, 8888, 1313]

    init() {
        Task { await observeTunnels() }
        Task { await pollPorts() }
        Task { await pollRelay() }
        Task { await startLaunchApps() }
        Task { await pollUpdates() }
    }

    // MARK: Derived

    var liveCount: Int { tunnels.values.filter { $0.status == .live }.count }
    var anyActive: Bool { tunnels.values.contains { $0.status.isActive } }
    var relayHost: String { TunnelOptions(relay: relay).relayEndpoint.host }
    var totalTraffic: UInt64 { tunnels.values.reduce(0) { $0 + $1.stats.bytesIn + $1.stats.bytesOut } }

    func tunnel(for app: SavedApp) -> TunnelInfo? { tunnels[app.id] }
    func isOn(_ app: SavedApp) -> Bool { starting.contains(app.id) || (tunnels[app.id]?.status.isActive ?? false) }

    // MARK: Tunnels

    func setOn(_ on: Bool, for app: SavedApp) {
        if on { Task { await start(app) } } else { stop(app) }
    }

    func start(_ app: SavedApp) async {
        guard !isOn(app) else { return }
        notices[app.id] = nil
        tunnels[app.id] = nil // Drop a previous error.
        starting.insert(app.id)
        let isListening = await PortProbe.isListening(app.port)
        listening[app.port] = isListening
        guard starting.remove(app.id) != nil else { return } // Switched off while checking.
        guard isListening else {
            withAnimation(.ui) { notices[app.id] = "Nothing running on :\(app.port)" }
            return
        }
        let options = TunnelOptions(relay: relay, secret: secret, autoReconnect: autoReconnect)
        await manager.start(id: app.id, localPort: app.port, remotePort: app.publicPort ?? 0, options: options)
    }

    func stop(_ app: SavedApp) {
        notices[app.id] = nil
        starting.remove(app.id)
        manager.stop(app.id)
        tunnels[app.id] = nil // Also clears a stopped tunnel's error.
    }

    func stopAll() { manager.stopAll() }

    private func observeTunnels() async {
        for await info in manager.updates() {
            let statusChanged = tunnels[info.id]?.status != info.status
            let apply = { self.tunnels[info.id] = info.status == .stopped ? nil : info }
            if statusChanged { withAnimation(.ui, apply) } else { apply() }
        }
    }

    private func startLaunchApps() async {
        for app in apps where app.startOnLaunch { await start(app) }
    }

    // MARK: Clipboard & browser

    /// Link visitors open: the custom address if set, otherwise http://relay:port.
    func webURL(_ app: SavedApp) -> URL? {
        guard tunnels[app.id]?.status == .live else { return nil }
        if let address = app.publicAddress { return URL(string: address) }
        return tunnels[app.id]?.webURL
    }

    /// Short form for display, without the scheme.
    func displayAddress(_ app: SavedApp) -> String? {
        guard let tunnel = tunnels[app.id], tunnel.status == .live else { return nil }
        guard let address = app.publicAddress else { return tunnel.publicURL }
        return address.replacingOccurrences(of: #"^https?://"#, with: "", options: .regularExpression)
    }

    func copyURL(_ app: SavedApp) {
        guard let url = webURL(app) else { return }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(url.absoluteString, forType: .string)
        copiedAt[app.id] = Date()
        objectWillChange.send()
    }

    func recentlyCopied(_ app: SavedApp) -> Bool {
        guard let date = copiedAt[app.id] else { return false }
        return Date().timeIntervalSince(date) < 1.4
    }

    func openInBrowser(_ app: SavedApp) {
        if let url = webURL(app) { NSWorkspace.shared.open(url) }
    }

    // MARK: Saved apps

    func add(_ app: SavedApp) {
        withAnimation(.ui) { apps.append(app) }
        persist()
        Task { await probe([app.port]) }
    }

    func update(_ app: SavedApp) {
        guard let index = apps.firstIndex(where: { $0.id == app.id }) else { return }
        let portChanged = apps[index].port != app.port
        // A running tunnel keeps its old ports until it is switched on again, so close it.
        if portChanged || apps[index].publicPort != app.publicPort { stop(apps[index]) }
        apps[index] = app
        persist()
        if portChanged { Task { await probe([app.port]) } }
    }

    func delete(_ app: SavedApp) {
        stop(app)
        notices[app.id] = nil
        withAnimation(.ui) { apps.removeAll { $0.id == app.id } }
        persist()
    }

    func move(from source: IndexSet, to destination: Int) {
        apps.move(fromOffsets: source, toOffset: destination)
        persist()
    }

    private func persist() {
        AppStore.save(apps)
        refreshDetected()
    }

    // MARK: Health

    private func pollPorts() async {
        while !Task.isCancelled {
            await probe(Array(Set(apps.map(\.port)).union(Self.commonPorts)))
            try? await Task.sleep(nanoseconds: 2_500_000_000)
        }
    }

    private func probe(_ ports: [UInt16]) async {
        let results = await withTaskGroup(of: (UInt16, Bool).self) { group in
            for port in ports { group.addTask { (port, await PortProbe.isListening(port, timeout: 0.6)) } }
            var results: [UInt16: Bool] = [:]
            for await (port, isListening) in group { results[port] = isListening }
            return results
        }
        for (port, isListening) in results where listening[port] != isListening {
            listening[port] = isListening
        }
        // A notice about a dead port is stale once something starts listening.
        for app in apps where results[app.port] == true && notices[app.id] != nil {
            notices[app.id] = nil
        }
        refreshDetected()
    }

    private func refreshDetected() {
        let saved = Set(apps.map(\.port))
        let detected = Self.commonPorts.filter { listening[$0] == true && !saved.contains($0) }
        if detected != detectedPorts { withAnimation(.ui) { detectedPorts = detected } }
    }

    private func pollRelay() async {
        while !Task.isCancelled {
            await refreshRelay()
            try? await Task.sleep(nanoseconds: 20_000_000_000)
        }
    }

    func refreshRelay() async {
        let endpoint = TunnelOptions(relay: relay).relayEndpoint
        let latency = await PortProbe.measure(host: endpoint.host, port: endpoint.port)
        relayLatency = latency
        relayReachable = latency != nil
    }

    // MARK: Updates

    private func pollUpdates() async {
        while !Task.isCancelled {
            await refreshUpdate()
            try? await Task.sleep(nanoseconds: 24 * 3600 * 1_000_000_000)
        }
    }

    func refreshUpdate() async {
        guard checkForUpdates else { return }
        let release = await UpdateChecker.check()
        if release != availableUpdate { withAnimation(.ui) { availableUpdate = release } }
    }

    // MARK: System

    func setLaunchAtLogin(_ enabled: Bool) {
        do {
            if enabled { try SMAppService.mainApp.register() } else { try SMAppService.mainApp.unregister() }
            launchAtLoginError = nil
        } catch {
            launchAtLoginError = error.localizedDescription
        }
        launchAtLogin = SMAppService.mainApp.status == .enabled
    }

    func applyActivationPolicy() {
        NSApp.setActivationPolicy(showDockIcon ? .regular : .accessory)
    }
}
