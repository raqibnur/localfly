import Foundation

public typealias TunnelID = UUID

public enum TunnelStatus: Equatable, Sendable {
    case connecting
    case live
    case error(String)
    case stopped

    public var isActive: Bool { self == .connecting || self == .live }
}

/// Where tunnels are opened, and how. `relay` accepts `host` or `host:port` (control port, default 7835),
/// so pointing the app at a self-hosted relay is a settings change.
public struct TunnelOptions: Equatable, Sendable {
    public var relay: String
    public var secret: String?
    public var localHost: String
    public var autoReconnect: Bool

    public init(relay: String = "bore.pub", secret: String? = nil, localHost: String = "localhost", autoReconnect: Bool = false) {
        self.relay = relay
        self.secret = secret
        self.localHost = localHost
        self.autoReconnect = autoReconnect
    }

    /// Relay host and control port parsed from `relay`.
    public var relayEndpoint: (host: String, port: UInt16) {
        let trimmed = relay.trimmingCharacters(in: .whitespacesAndNewlines)
        if let colon = trimmed.lastIndex(of: ":"), let port = UInt16(trimmed[trimmed.index(after: colon)...]) {
            return (String(trimmed[..<colon]), port)
        }
        return (trimmed.isEmpty ? "bore.pub" : trimmed, BoreProtocol.controlPort)
    }
}

public struct TunnelInfo: Identifiable, Equatable, Sendable {
    public let id: TunnelID
    public let localPort: UInt16
    public let relayHost: String
    public var remotePort: UInt16?
    public var status: TunnelStatus
    public var liveSince: Date?
    public var stats = TunnelStats()
    /// Reconnect attempts since the tunnel was last live.
    public var retryAttempt = 0

    /// `relay:remote_port`, the address visitors use.
    public var publicURL: String? { remotePort.map { "\(relayHost):\($0)" } }
    /// The same address as an `http://` link.
    public var webURL: URL? { publicURL.flatMap { URL(string: "http://\($0)") } }
}

/// Owns every running tunnel. Main-actor isolated so UI layers can read it directly.
@MainActor
public final class TunnelManager {
    private struct Handle {
        var info: TunnelInfo
        var options: TunnelOptions
        /// Public port asked of the relay; 0 lets the relay pick.
        var fixedPort: UInt16
        var client: BoreClient?
        var generation = 0
        var retry: Task<Void, Never>?
        var waiters: [CheckedContinuation<TunnelInfo, Never>] = []
    }

    private var handles: [TunnelID: Handle] = [:]
    private var subscribers: [UUID: AsyncStream<TunnelInfo>.Continuation] = [:]

    public init() {}

    /// Every status or stats change, for every tunnel.
    public func updates() -> AsyncStream<TunnelInfo> {
        let key = UUID()
        return AsyncStream { continuation in
            subscribers[key] = continuation
            continuation.onTermination = { [weak self] _ in
                Task { @MainActor in self?.subscribers[key] = nil }
            }
        }
    }

    /// Opens a tunnel and returns once it is live or the first attempt failed.
    /// Pass `id` to key the tunnel by your own identifier; an existing tunnel with that id is replaced.
    /// Pass `remotePort` to ask the relay for that public port (it must be free and inside the relay's range),
    /// so a reverse proxy in front of the relay can map a fixed hostname to it.
    @discardableResult
    public func start(id: TunnelID = TunnelID(), localPort: UInt16, remotePort: UInt16 = 0, options: TunnelOptions = TunnelOptions()) async -> TunnelInfo {
        stop(id)
        let info = TunnelInfo(id: id, localPort: localPort, relayHost: options.relayEndpoint.host, status: .connecting)
        handles[id] = Handle(info: info, options: options, fixedPort: remotePort)
        publish(info)
        return await withCheckedContinuation { continuation in
            handles[id]?.waiters.append(continuation)
            connect(id, requestedPort: remotePort)
        }
    }

    /// Closes the tunnel and every visitor connection immediately.
    public func stop(_ id: TunnelID) {
        guard var handle = handles.removeValue(forKey: id) else { return }
        handle.client?.cancel()
        handle.retry?.cancel()
        handle.info.status = .stopped
        handle.info.stats.activeConnections = 0
        resume(&handle)
        publish(handle.info)
    }

    public func stopAll() {
        for id in handles.keys { stop(id) }
    }

    public func list() -> [TunnelInfo] {
        handles.values.map(\.info).sorted { $0.localPort < $1.localPort }
    }

    public func info(_ id: TunnelID) -> TunnelInfo? { handles[id]?.info }

    // MARK: Lifecycle

    private func connect(_ id: TunnelID, requestedPort: UInt16) {
        guard var handle = handles[id] else { return }
        handle.generation += 1
        let generation = handle.generation
        let relay = handle.options.relayEndpoint
        let config = BoreClient.Config(
            localHost: handle.options.localHost,
            localPort: handle.info.localPort,
            relayHost: relay.host,
            relayPort: relay.port,
            secret: handle.options.secret.flatMap { $0.isEmpty ? nil : $0 },
            requestedPort: requestedPort
        )
        let client = BoreClient(config: config) { [weak self] event in
            // Hop in FIFO order so status changes are never reordered.
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.handle(event, for: id, generation: generation) }
            }
        }
        handle.client = client
        handles[id] = handle
        client.start()
    }

    private func handle(_ event: BoreClient.Event, for id: TunnelID, generation: Int) {
        guard var handle = handles[id], handle.generation == generation else { return }
        switch event {
        case .live(let remotePort):
            handle.info.remotePort = remotePort
            handle.info.status = .live
            handle.info.liveSince = Date()
            handle.info.retryAttempt = 0
            handle.info.stats = TunnelStats()
            resume(&handle)
        case .stats(let stats):
            handle.info.stats = stats
        case .failed(let reason):
            handle.client = nil
            handle.info.liveSince = nil
            handle.info.stats.activeConnections = 0
            handle.info.status = .error(reason)
            resume(&handle) // A caller awaiting start() learns why the first attempt failed.
            if handle.options.autoReconnect {
                handle.info.status = .connecting
                // A fixed port is always asked for again; otherwise the first retry tries to keep the old one.
                let port = handle.fixedPort != 0 ? handle.fixedPort : handle.info.retryAttempt == 0 ? (handle.info.remotePort ?? 0) : 0
                handle.retry = scheduleRetry(id, attempt: handle.info.retryAttempt, requestedPort: port)
                handle.info.retryAttempt += 1
            }
        }
        handles[id] = handle
        publish(handle.info)
    }

    /// Exponential backoff: 1, 2, 4 … 30 s.
    private func scheduleRetry(_ id: TunnelID, attempt: Int, requestedPort port: UInt16) -> Task<Void, Never> {
        let delay = min(30.0, pow(2.0, Double(attempt)))
        return Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(delay * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.connect(id, requestedPort: port)
        }
    }

    private func resume(_ handle: inout Handle) {
        for waiter in handle.waiters { waiter.resume(returning: handle.info) }
        handle.waiters.removeAll()
    }

    private func publish(_ info: TunnelInfo) {
        for subscriber in subscribers.values { subscriber.yield(info) }
    }
}
